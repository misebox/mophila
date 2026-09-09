//! Timeline のテキスト出力。何が、いつ、どう変わるかを時刻順に並べる。LLM や人が読んで見直すためのもの

use std::collections::HashMap;
use std::rc::Rc;

use crate::lang::eval::Interp;
use crate::render::media::Media;
use crate::lang::value::{ObjRef, Placed, Timeline, Track, Value};

/// 1 つの変化。区間 (from..to) で attr が v0 から v1 に変わる
pub struct Event {
    pub from: f64,
    pub to: f64,
    pub target: ObjRef,
    pub attr: String,
    pub v0: Option<Value>,
    pub v1: Value,
    pub ease: Option<String>,
    pub effect: Option<String>,
}

/// --filter key=value の条件
#[derive(Default)]
pub struct Filter {
    pub kind: Option<String>,
    pub attr: Option<String>,
    pub text: Option<String>,
    pub from: Option<f64>,
    pub to: Option<f64>,
}

impl Filter {
    pub fn parse(items: &[String]) -> Result<Self, String> {
        let mut f = Filter::default();
        for item in items {
            let Some((k, v)) = item.split_once('=') else {
                return Err(format!("filter must be key=value: {item}"));
            };
            match k {
                "kind" => f.kind = Some(v.to_string()),
                "attr" => f.attr = Some(v.to_string()),
                "text" => f.text = Some(v.to_string()),
                "from" => f.from = Some(crate::parse_duration(v)?),
                "to" => f.to = Some(crate::parse_duration(v)?),
                other => return Err(format!("unknown filter key: {other} (kind, attr, text, from, to)")),
            }
        }
        Ok(f)
    }

    fn accepts(&self, e: &Event) -> bool {
        let o = e.target.borrow();
        if self.kind.as_deref().is_some_and(|k| k != o.kind) {
            return false;
        }
        if self.attr.as_deref().is_some_and(|a| a != e.attr) {
            return false;
        }
        if let Some(t) = &self.text {
            let content = match o.attrs.get("text") {
                Some(Value::Str(s)) => s.clone(),
                _ => String::new(),
            };
            if !content.contains(t.as_str()) {
                return false;
            }
        }
        if self.from.is_some_and(|f| e.to < f) || self.to.is_some_and(|t| e.from > t) {
            return false;
        }
        true
    }
}

/// View に置かれた Timeline を辿り、絶対時刻の変化の列にする
pub fn collect(interp: &mut Interp, view: &ObjRef) -> Vec<Event> {
    let mut events = Vec::new();
    let tracks = view.borrow().tracks.clone();
    for placed in &tracks {
        walk(interp, placed, 0.0, &mut events);
    }
    events.sort_by(|a, b| a.from.partial_cmp(&b.from).unwrap_or(std::cmp::Ordering::Equal));
    events
}

fn walk(interp: &mut Interp, placed: &Placed, origin: f64, out: &mut Vec<Event>) {
    let start = origin + placed.at;
    match &placed.track {
        Track::Container(obj) => {
            let children = obj.borrow().tracks.clone();
            for child in &children {
                walk(interp, child, start, out);
            }
        }
        Track::Timeline(tl) => timeline_events(interp, tl, start, out),
        Track::Audio(..) | Track::Subtitle(_) => {}
    }
}

/// 字幕と音声を時刻順に。字幕は文字数から見て短いものに SHORT を付ける
pub fn media_report(media: &Media, filter: &Filter) -> String {
    let in_range = |from: f64, to: f64| !(filter.from.is_some_and(|x| to < x) || filter.to.is_some_and(|x| from > x));
    let mut rows: Vec<(f64, String)> = Vec::new();
    if filter.kind.as_deref().is_none_or(|k| k == "Subtitle") {
        for cue in &media.cues {
            let to = cue.at + cue.length;
            if !in_range(cue.at, to) || filter.text.as_deref().is_some_and(|t| !cue.text.contains(t)) {
                continue;
            }
            let chars = cue.text.chars().filter(|c| !c.is_whitespace()).count();
            let needed = (chars as f64 * 0.25).max(1.5);
            let note = if cue.length < needed { format!("  SHORT (needs {needed:.1}s)") } else { String::new() };
            rows.push((cue.at, format!("{:>8.2}s – {:>7.2}s  {:>6.1}s  Subtitle \"{}\"{note}", cue.at, to, cue.length, truncate(&cue.text.replace('\n', " / "), 40))));
        }
    }
    if filter.kind.as_deref().is_none_or(|k| k == "Audio") && filter.text.is_none() {
        for clip in &media.clips {
            let to = clip.at + clip.length;
            if !in_range(clip.at, to) {
                continue;
            }
            let mut notes = Vec::new();
            if clip.looping {
                notes.push("loop".to_string());
            }
            if clip.volume != 1.0 {
                notes.push(format!("volume {}", clip.volume));
            }
            if clip.fade_in > 0.0 {
                notes.push(format!("fadeIn {}s", clip.fade_in));
            }
            if clip.fade_out > 0.0 {
                notes.push(format!("fadeOut {}s", clip.fade_out));
            }
            rows.push((clip.at, format!("{:>8.2}s – {:>7.2}s  {:>6.1}s  Audio {:?}  {}", clip.at, to, clip.length, clip.name, notes.join(", "))));
        }
    }
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    rows.into_iter().map(|(_, s)| s).collect::<Vec<_>>().join("\n")
}

fn timeline_events(interp: &mut Interp, tl: &Rc<Timeline>, start: f64, out: &mut Vec<Event>) {
    let scale = tl.time_scale();
    // (対象, 属性パス) ごとに、時刻順の (時刻, 値, ease, effect)
    let mut series: Vec<(ObjRef, String, Vec<(f64, Option<f64>, Value, Option<String>, Option<String>)>)> = Vec::new();
    for kf in &tl.keyframes {
        for a in &kf.assigns {
            let path = a.path.join(".");
            let value = interp.eval_assign_pub(tl, a, kf.time).unwrap_or(Value::Nothing);
            let entry = (start + kf.time * scale, kf.end.map(|e| start + e * scale), value, kf.ease.clone(), kf.effect.clone());
            match series.iter_mut().find(|(o, p, _)| Rc::ptr_eq(o, &a.target) && *p == path) {
                Some((_, _, list)) => list.push(entry),
                None => series.push((a.target.clone(), path, vec![entry])),
            }
        }
    }
    for (target, attr, list) in series {
        let mut prev: Option<&(f64, Option<f64>, Value, Option<String>, Option<String>)> = None;
        for item in &list {
            let (time, end, value, ease, effect) = item;
            match end {
                Some(e) => out.push(Event { from: *time, to: *e, target: target.clone(), attr: attr.clone(), v0: None, v1: value.clone(), ease: None, effect: Some("continuous".into()) }),
                None => out.push(Event {
                    from: prev.map_or(*time, |p| p.1.unwrap_or(p.0)),
                    to: *time,
                    target: target.clone(),
                    attr: attr.clone(),
                    v0: prev.map(|p| p.2.clone()),
                    v1: value.clone(),
                    ease: ease.clone(),
                    effect: effect.clone(),
                }),
            }
            prev = Some(item);
        }
    }
}

fn label(o: &ObjRef) -> String {
    let b = o.borrow();
    match b.attrs.get("text") {
        Some(Value::Str(s)) => format!("{} \"{}\"", b.kind, truncate(s, 24)),
        _ => b.kind.clone(),
    }
}

fn truncate(s: &str, n: usize) -> String {
    let mut out: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        out.push('…');
    }
    out
}

/// 変化の列を行にする
pub fn format_events(events: &[Event], filter: &Filter) -> String {
    let mut lines = Vec::new();
    for e in events.iter().filter(|e| filter.accepts(e)) {
        let change = match &e.v0 {
            Some(v0) if e.from < e.to => format!("{v0} → {}", e.v1),
            _ => format!("= {}", e.v1),
        };
        let mods: String = [&e.ease, &e.effect].iter().filter_map(|m| m.as_ref()).map(|m| format!(" :{m}")).collect();
        lines.push(format!("{:>8.2}s  {:>7.2}s  {:<40} {} {}{}", e.from, e.to - e.from, label(&e.target), e.attr, change, mods));
    }
    lines.join("\n")
}

/// TextArea ごとに、opacity が 0 より大きい区間と、文字数から見た読める時間の目安
pub fn text_visibility(events: &[Event], filter: &Filter) -> String {
    let mut by_obj: HashMap<usize, (ObjRef, Vec<(f64, f64)>)> = HashMap::new();
    for e in events.iter().filter(|e| e.attr == "opacity" && e.target.borrow().kind == "TextArea") {
        let entry = by_obj.entry(Rc::as_ptr(&e.target) as usize).or_insert_with(|| (e.target.clone(), Vec::new()));
        let v1 = match e.v1 { Value::Number(n) => n, _ => 0.0 };
        entry.1.push((e.to, v1));
    }
    let mut rows: Vec<(f64, String)> = Vec::new();
    for (_, (obj, mut points)) in by_obj {
        points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let text = match obj.borrow().attrs.get("text") {
            Some(Value::Str(s)) => s.clone(),
            _ => String::new(),
        };
        if filter.text.as_deref().is_some_and(|t| !text.contains(t)) || filter.kind.as_deref().is_some_and(|k| k != "TextArea") {
            continue;
        }
        let chars = text.chars().filter(|c| !c.is_whitespace()).count();
        let needed = (chars as f64 * 0.25).max(1.5);
        let mut visible_from: Option<f64> = None;
        for (time, v) in &points {
            match (visible_from, *v > 0.0) {
                (None, true) => visible_from = Some(*time),
                (Some(f), false) => {
                    let span = time - f;
                    let in_range = !(filter.from.is_some_and(|x| *time < x) || filter.to.is_some_and(|x| f > x));
                    if in_range {
                        let note = if span < needed { format!("  SHORT (needs {needed:.1}s)") } else { String::new() };
                        rows.push((f, format!("{:>8.2}s – {:>7.2}s  {:>6.1}s  \"{}\"{note}", f, time, span, truncate(&text, 40))));
                    }
                    visible_from = None;
                }
                _ => {}
            }
        }
        if let Some(f) = visible_from {
            if !filter.to.is_some_and(|x| f > x) {
                rows.push((f, format!("{:>8.2}s – (end)     \"{}\"", f, truncate(&text, 40))));
            }
        }
    }
    rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    rows.into_iter().map(|(_, s)| s).collect::<Vec<_>>().join("\n")
}
