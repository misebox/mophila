//! mophila.yaml — プロジェクトの設定。
//!
//! 実行したディレクトリにあれば自動で読む。`-f` で別の場所を指せる。
//! 設定の中に書いたパスは **設定ファイルのある場所から** の相対。
//! コマンドラインに書いたパスは、いま居るディレクトリからの相対 (シェルで打ったとおり)。

use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};

use crate::lang::value::Value;

pub const FILE_NAME: &str = "mophila.yaml";

/// "a/./b" や先頭の "./" を落として読みやすくする。ファイルの有無は見ない
fn tidy(path: PathBuf) -> PathBuf {
    let parts: Vec<_> = path.components().filter(|c| !matches!(c, std::path::Component::CurDir)).collect();
    match parts.is_empty() {
        true => PathBuf::from("."),
        false => parts.iter().collect(),
    }
}

/// 設定に書ける値。YAML のスカラーのうち 3 つだけ扱う
#[derive(Debug, Clone, PartialEq)]
pub enum Setting {
    Number(f64),
    Str(String),
    Bool(bool),
}

impl Setting {
    pub fn type_name(&self) -> &'static str {
        match self {
            Setting::Number(_) => "Number",
            Setting::Str(_) => "String",
            Setting::Bool(_) => "Bool",
        }
    }

    pub fn to_value(&self) -> Value {
        match self {
            Setting::Number(n) => Value::num(*n),
            Setting::Str(s) => Value::Str(s.clone()),
            Setting::Bool(b) => Value::Bool(*b),
        }
    }

    /// 文字列で来た上書き (環境変数・--set) を、元の型に合わせて読む
    fn parse_like(&self, text: &str, from: &str, name: &str) -> Result<Setting, Box<dyn Error>> {
        let wrong = || format!("{from} で {name} に \"{text}\" が来たが、{} が要る", self.type_name());
        Ok(match self {
            Setting::Number(_) => Setting::Number(text.parse().map_err(|_| wrong())?),
            Setting::Bool(_) => Setting::Bool(match text {
                "true" | "yes" | "on" | "1" => true,
                "false" | "no" | "off" | "0" => false,
                _ => return Err(wrong().into()),
            }),
            Setting::Str(_) => Setting::Str(text.to_string()),
        })
    }
}

pub struct Project {
    /// 設定ファイルそのもの。ログに出す
    pub path: PathBuf,
    /// `@/` が指す場所
    pub root: PathBuf,
    /// `@name` が指す場所
    pub aliases: HashMap<String, PathBuf>,
    /// `import config` で読める値。書いた順を保つ
    pub config: Vec<(String, Setting)>,
    /// 既定のスクリプト
    pub entry: Option<PathBuf>,
}

impl Project {
    /// 設定ファイルを読む。`given` が無ければ、いま居るディレクトリの mophila.yaml だけを見る
    pub fn load(given: Option<&str>, overrides: &[String]) -> Result<Option<Self>, Box<dyn Error>> {
        let path = match given {
            Some(p) => PathBuf::from(p),
            None => match Path::new(FILE_NAME).exists() {
                true => PathBuf::from(FILE_NAME),
                false => return Ok(None),
            },
        };
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{} を読めない: {e}", path.display()))?;
        let doc: serde_yml::Value = serde_yml::from_str(&text).map_err(|e| format!("{} を読めない: {e}", path.display()))?;
        let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).map_or_else(|| PathBuf::from("."), Path::to_path_buf);

        let at = |key: &str| doc.get(key).cloned();
        let as_path = |v: &serde_yml::Value, key: &str| -> Result<PathBuf, Box<dyn Error>> {
            match v {
                serde_yml::Value::String(s) => Ok(tidy(dir.join(s))),
                _ => Err(format!("{}: {key} はパスの文字列で書く", path.display()).into()),
            }
        };

        let root = match at("root") {
            Some(v) => tidy(as_path(&v, "root")?),
            None => dir.clone(),
        };
        let entry = match at("entry") {
            Some(v) => Some(as_path(&v, "entry")?),
            None => None,
        };
        let mut aliases = HashMap::new();
        if let Some(map) = at("aliases") {
            let Some(map) = map.as_mapping() else {
                return Err(format!("{}: aliases は 名前: パス の並びで書く", path.display()).into());
            };
            for (k, v) in map {
                let name = k.as_str();
                if name == "/" {
                    return Err(format!("{}: \"/\" は root のことなので aliases に書けない", path.display()).into());
                }
                aliases.insert(name.to_string(), as_path(v, &format!("aliases.{name}"))?);
            }
        }
        let mut config = Vec::new();
        if let Some(map) = at("config") {
            let Some(map) = map.as_mapping() else {
                return Err(format!("{}: config は 名前: 値 の並びで書く", path.display()).into());
            };
            for (k, v) in map {
                let name = k.as_str();
                let value = match v {
                    serde_yml::Value::Bool(b) => Setting::Bool(*b),
                    serde_yml::Value::Number(n) => Setting::Number(n.as_f64()),
                    serde_yml::Value::String(s) => Setting::Str(s.clone()),
                    _ => return Err(format!("{}: config.{name} は数・文字列・真偽のどれかで書く", path.display()).into()),
                };
                config.push((name.to_string(), value));
            }
        }
        let mut project = Project { path, root, aliases, config, entry };
        project.apply_env()?;
        project.apply_overrides(overrides)?;
        Ok(Some(project))
    }

    /// MOPHILA_<大文字の名前> があれば上書きする
    fn apply_env(&mut self) -> Result<(), Box<dyn Error>> {
        for (name, value) in &mut self.config {
            let key = format!("MOPHILA_{}", name.to_uppercase());
            if let Ok(text) = std::env::var(&key) {
                *value = value.parse_like(&text, &key, name)?;
            }
        }
        Ok(())
    }

    /// --set 名前=値。設定に無い名前はエラー (打ち間違いを黙って捨てない)
    fn apply_overrides(&mut self, overrides: &[String]) -> Result<(), Box<dyn Error>> {
        for one in overrides {
            let Some((name, text)) = one.split_once('=') else {
                return Err(format!("--set は 名前=値 の形で書く: \"{one}\"").into());
            };
            let Some((_, value)) = self.config.iter_mut().find(|(n, _)| n == name) else {
                let known: Vec<&str> = self.config.iter().map(|(n, _)| n.as_str()).collect();
                return Err(format!("--set {name}: config に \"{name}\" が無い。あるのは {}", known.join(" ")).into());
            };
            *value = value.parse_like(text, "--set", name)?;
        }
        Ok(())
    }

    /// "@/a/b.moph" や "@utils/format.moph" を実際のパスにする。@ で始まらなければそのまま
    pub fn resolve(&self, path: &str) -> Result<PathBuf, Box<dyn Error>> {
        let Some(rest) = path.strip_prefix('@') else { return Ok(PathBuf::from(path)) };
        let (name, tail) = match rest.split_once('/') {
            Some((name, tail)) => (name, tail),
            None => (rest, ""),
        };
        let base = match name {
            "" => &self.root,
            name => self
                .aliases
                .get(name)
                .ok_or_else(|| format!("\"@{name}\" は mophila.yaml の aliases に無い。あるのは {}", self.alias_names()))?,
        };
        Ok(base.join(tail))
    }

    fn alias_names(&self) -> String {
        let mut names: Vec<&str> = self.aliases.keys().map(String::as_str).collect();
        names.sort_unstable();
        match names.is_empty() {
            true => "@/ だけ".to_string(),
            false => format!("@/ と {}", names.iter().map(|n| format!("@{n}")).collect::<Vec<_>>().join(" ")),
        }
    }

    /// 読んだことが分かるように 1 行出す
    pub fn announce(&self) {
        let names: Vec<&str> = self.config.iter().map(|(n, _)| n.as_str()).collect();
        let settings = match names.is_empty() {
            true => String::new(),
            false => format!(", config {}", names.join(" ")),
        };
        eprintln!("{} を読んだ (root {}{settings})", self.path.display(), self.root.display());
    }
}
