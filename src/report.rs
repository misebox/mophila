//! Layer のテキスト出力。何が、いつからいつまで存在するかを時刻順に並べる。LLM や人が読んで見直すためのもの

use crate::lang::eval::Interp;
use crate::lang::value::{ObjRef, Value};
use crate::render::media::Media;

/// 置かれた Layer 1 つ。時刻は動画の時間軸での絶対時刻
pub struct Row {
    pub from: f64,
    pub to: f64,
    /// 入れ子の Composition の中は "intro/title" のように道筋を付ける
    pub name: String,
    /// Circle や Group などの型の名前。関数なら、時刻 0 で呼んで返った Node の型を `func → Group` の形で
    pub kind: String,
    /// 中にある TextArea の文
    pub texts: Vec<String>,
}

/// --filter key=value の条件
#[derive(Default)]
pub struct Filter {
    pub kind: Option<String>,
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
                "text" => f.text = Some(v.to_string()),
                "from" => f.from = Some(crate::parse_duration(v)?),
                "to" => f.to = Some(crate::parse_duration(v)?),
                other => return Err(format!("unknown filter key: {other} (kind, text, from, to)")),
            }
        }
        Ok(f)
    }

    fn accepts(&self, row: &Row) -> bool {
        if self.kind.as_deref().is_some_and(|k| !row.kind.contains(k)) {
            return false;
        }
        if self.text.as_deref().is_some_and(|t| !row.texts.iter().any(|s| s.contains(t))) {
            return false;
        }
        !(self.from.is_some_and(|f| row.to < f) || self.to.is_some_and(|t| row.from > t))
    }
}

/// Composition に置かれた Layer を辿り、絶対時刻の行にする
pub fn collect(interp: &mut Interp, comp: &ObjRef) -> Vec<Row> {
    let mut rows = Vec::new();
    walk(interp, comp, "", 0.0, f64::INFINITY, &mut rows);
    rows.sort_by(|a, b| a.from.total_cmp(&b.from));
    rows
}

fn walk(interp: &mut Interp, comp: &ObjRef, prefix: &str, origin: f64, limit: f64, out: &mut Vec<Row>) {
    let layers = comp.borrow().layers.clone();
    for layer in &layers {
        let (from, to) = (origin + layer.at, (origin + layer.out).min(limit));
        let name = format!("{prefix}{}", layer.name);
        let (kind, texts) = match &layer.content {
            Value::Object(o) if o.borrow().kind == "Composition" => {
                walk(interp, o, &format!("{name}/"), from, to, out);
                ("Composition".to_string(), Vec::new())
            }
            Value::Object(o) => (o.borrow().kind.clone(), texts_in(o)),
            Value::Func(f) => {
                // 時刻 0 と真ん中で呼んで、出てきた文を集める。Layer の中の時刻で変わる文は拾いきれない
                let mut texts = Vec::new();
                let mut kind = "func".to_string();
                for local in [0.0, (layer.out - layer.at) / 2.0] {
                    if let Ok(Value::Object(node)) = interp.apply(f, vec![(None, Value::Duration(local))]) {
                        if local == 0.0 {
                            kind = format!("func → {}", node.borrow().kind);
                        }
                        for text in texts_in(&node) {
                            if !texts.contains(&text) {
                                texts.push(text);
                            }
                        }
                    }
                }
                (kind, texts)
            }
            Value::Audio(_) => ("Audio".to_string(), Vec::new()),
            _ => ("?".to_string(), Vec::new()),
        };
        out.push(Row { from, to, name, kind, texts });
    }
}

/// 図形と、Group の中の図形にある TextArea の文
fn texts_in(node: &ObjRef) -> Vec<String> {
    let n = node.borrow();
    let mut out = Vec::new();
    if n.kind == "TextArea" {
        if let Some(Value::Str(s)) = n.attrs.get("text") {
            out.push(s.clone());
        }
    }
    for child in &n.children {
        out.extend(texts_in(child));
    }
    out
}

/// 読み上げと音声を時刻順に
pub fn media_report(media: &Media, filter: &Filter) -> String {
    let in_range = |from: f64, to: f64| !(filter.from.is_some_and(|x| to < x) || filter.to.is_some_and(|x| from > x));
    let mut rows: Vec<(f64, String)> = Vec::new();
    if filter.kind.as_deref().is_none_or(|k| k == "Narration") {
        for line in &media.narrations {
            let to = line.at + line.length;
            if !in_range(line.at, to) || filter.text.as_deref().is_some_and(|t| !line.text.contains(t)) {
                continue;
            }
            // どの engine のどの声で読むかを出す
            let how: Vec<String> = line
                .engine
                .iter()
                .cloned()
                .chain(line.settings.0.iter().map(|(n, v)| format!("{n}={v}")))
                .collect();
            let voice = match how.is_empty() {
                true => String::new(),
                false => format!(" ({})", how.join(" ")),
            };
            rows.push((line.at, format!("{:>8.2}s – {:>7.2}s  {:>6.1}s  Narration{voice} \"{}\"", line.at, to, line.length, truncate(&line.text.replace('\n', " / "), 40))));
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
            rows.push((clip.at, format!("{:>8.2}s – {:>7.2}s  {:>6.1}s  Audio {:?}  {}", clip.at, to, clip.length, clip.name, notes.join(", "))));
        }
    }
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    rows.into_iter().map(|(_, s)| s).collect::<Vec<_>>().join("\n")
}

fn truncate(s: &str, n: usize) -> String {
    let mut out: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        out.push('…');
    }
    out
}

/// Layer を行にする
pub fn format_layers(rows: &[Row], filter: &Filter) -> String {
    rows.iter()
        .filter(|r| filter.accepts(r))
        .map(|r| {
            let text = match r.texts.first() {
                Some(t) => format!(" \"{}\"", truncate(&t.replace('\n', " / "), 24)),
                None => String::new(),
            };
            format!("{:>8.2}s – {:>7.2}s  {:>6.1}s  {:<24} {}{text}", r.from, r.to, r.to - r.from, r.name, r.kind)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// TextArea の文ごとに、出ている区間と、文字数から見た読める時間の目安
pub fn text_visibility(rows: &[Row], filter: &Filter) -> String {
    let mut lines: Vec<(f64, String)> = Vec::new();
    for row in rows.iter().filter(|r| filter.accepts(r)) {
        for text in &row.texts {
            if filter.text.as_deref().is_some_and(|t| !text.contains(t)) {
                continue;
            }
            let span = row.to - row.from;
            let chars = text.chars().filter(|c| !c.is_whitespace()).count();
            let needed = (chars as f64 * 0.25).max(1.5);
            let note = if span < needed { format!("  SHORT (needs {needed:.1}s)") } else { String::new() };
            lines.push((row.from, format!("{:>8.2}s – {:>7.2}s  {:>6.1}s  \"{}\"{note}", row.from, row.to, span, truncate(text, 40))));
        }
    }
    lines.sort_by(|a, b| a.0.total_cmp(&b.0));
    lines.into_iter().map(|(_, s)| s).collect::<Vec<_>>().join("\n")
}
