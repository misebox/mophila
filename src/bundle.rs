//! スクリプトと、import で辿れるファイルを実行ファイルの末尾に埋め込む。
//! 形式: [元の実行ファイル][アーカイブ][アーカイブ長 u64 LE][MAGIC]
//! アーカイブ: [件数 u32][パス長 u32][パス][本文長 u64][本文]... 先頭がメインのスクリプト。
//! 積むのは import で実際に辿れるファイルだけ (ファイル単位のツリーシェイキング)

use std::collections::HashMap;
use std::error::Error;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::lang::lexer::{Tok, lex};

const MAGIC: &[u8] = b"MOPHILA-BUNDLE-2";

/// 埋め込まれたファイル。パスはメインのスクリプトのディレクトリからの相対 (正規化済み)。
/// スクリプトは本文で持ち、音声などは一時ディレクトリに取り出して実際のパスで持つ
pub struct Sources {
    pub main: String,
    pub files: HashMap<String, String>,
    pub assets: HashMap<String, PathBuf>,
    temp_dir: Option<PathBuf>,
}

impl Sources {
    /// 取り出した音声などを消す
    pub fn cleanup(&self) {
        if let Some(dir) = &self.temp_dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// 自分自身をコピーし、末尾にスクリプトと依存ファイルを付けて output に書く
pub fn write(script: &Path, output: &str) -> Result<(), Box<dyn Error>> {
    let (main, files) = collect(script)?;
    let exe = std::fs::read(std::env::current_exe()?)?;
    let base = &exe[..payload_start(&exe).unwrap_or(exe.len())];

    let mut archive = Vec::new();
    let mut entries: Vec<(&String, &Vec<u8>)> = files.iter().collect();
    entries.sort_by(|a, b| (a.0 != &main).cmp(&(b.0 != &main)).then(a.0.cmp(b.0)));
    archive.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (path, content) in entries {
        archive.extend_from_slice(&(path.len() as u32).to_le_bytes());
        archive.extend_from_slice(path.as_bytes());
        archive.extend_from_slice(&(content.len() as u64).to_le_bytes());
        archive.extend_from_slice(content);
    }

    let mut file = std::fs::File::create(output)?;
    file.write_all(base)?;
    file.write_all(&archive)?;
    file.write_all(&(archive.len() as u64).to_le_bytes())?;
    file.write_all(MAGIC)?;
    drop(file);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(output, std::fs::Permissions::from_mode(0o755))?;
    }
    eprintln!("bundled {} file(s)", files.len());
    Ok(())
}

/// 自分自身に埋め込まれたスクリプトがあれば返す
pub fn embedded() -> Result<Option<Sources>, Box<dyn Error>> {
    let exe = std::fs::read(std::env::current_exe()?)?;
    let Some(start) = payload_start(&exe) else { return Ok(None) };
    let end = exe.len() - MAGIC.len() - 8;
    let mut cursor = &exe[start..end];
    let count = read_u32(&mut cursor)?;
    let mut files = HashMap::new();
    let mut assets = HashMap::new();
    let mut temp_dir = None;
    let mut main = None;
    for _ in 0..count {
        let path_len = read_u32(&mut cursor)? as usize;
        let path = String::from_utf8(cursor[..path_len].to_vec())?;
        cursor = &cursor[path_len..];
        let len = u64::from_le_bytes(cursor[..8].try_into()?) as usize;
        cursor = &cursor[8..];
        let content = &cursor[..len];
        cursor = &cursor[len..];
        if path.ends_with(".moph") {
            main.get_or_insert(path.clone());
            files.insert(path, String::from_utf8(content.to_vec())?);
            continue;
        }
        // 音声などは ffmpeg がファイルとして読むので、一時ディレクトリに取り出す
        let dir = match &temp_dir {
            Some(d) => d,
            None => {
                let d = std::env::temp_dir().join(format!("mophila-{}", std::process::id()));
                std::fs::create_dir_all(&d)?;
                temp_dir.insert(d)
            }
        };
        let file = dir.join(format!("{}-{}", assets.len(), Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or("asset")));
        std::fs::write(&file, content)?;
        assets.insert(path, file);
    }
    let main = main.ok_or("empty bundle")?;
    Ok(Some(Sources { main, files, assets, temp_dir }))
}

fn read_u32(cursor: &mut &[u8]) -> Result<u32, Box<dyn Error>> {
    let v = u32::from_le_bytes(cursor[..4].try_into()?);
    *cursor = &cursor[4..];
    Ok(v)
}

fn payload_start(exe: &[u8]) -> Option<usize> {
    if !exe.ends_with(MAGIC) || exe.len() < MAGIC.len() + 8 {
        return None;
    }
    let len_at = exe.len() - MAGIC.len() - 8;
    let len = u64::from_le_bytes(exe[len_at..len_at + 8].try_into().ok()?) as usize;
    len_at.checked_sub(len)
}

/// メインのスクリプトから import を辿り、到達したファイル (スクリプトと音声) だけを集める
fn collect(script: &Path) -> Result<(String, HashMap<String, Vec<u8>>), Box<dyn Error>> {
    let root = script.parent().map(Path::to_path_buf).unwrap_or_default();
    let main = normalize(Path::new(script.file_name().ok_or("script has no file name")?));
    let mut files = HashMap::new();
    let mut pending = vec![main.clone()];
    while let Some(rel) = pending.pop() {
        if files.contains_key(&rel) {
            continue;
        }
        let full = root.join(&rel);
        let bytes = std::fs::read(&full).map_err(|e| format!("cannot read {}: {e}", full.display()))?;
        if rel.ends_with(".moph") {
            let src = String::from_utf8(bytes.clone())?;
            let dir = Path::new(&rel).parent().map(Path::to_path_buf).unwrap_or_default();
            for target in imports_in(&src)? {
                pending.push(normalize(&dir.join(target)));
            }
        }
        files.insert(rel, bytes);
    }
    Ok((main, files))
}

/// ソースの中の import を、そのファイルからの相対パスにして返す。
/// 形: import .a.b / import ..a / import "file" / import { x } from <同上>
fn imports_in(src: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let tokens = lex(src)?;
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i].tok != Tok::Import {
            i += 1;
            continue;
        }
        i += 1;
        // { a, b } from を読み飛ばす
        if tokens[i].tok == Tok::LBrace {
            while i < tokens.len() && tokens[i].tok != Tok::RBrace {
                i += 1;
            }
            i += 1;
            if matches!(&tokens.get(i).map(|t| &t.tok), Some(Tok::Ident(k)) if k == "from") {
                i += 1;
            }
        }
        match &tokens[i].tok {
            Tok::Str(path) => out.push(path.clone()),
            Tok::Dot | Tok::DotDot => {
                let mut up = 0;
                loop {
                    match tokens[i].tok {
                        Tok::Dot => {
                            i += 1;
                            break;
                        }
                        Tok::DotDot => {
                            i += 1;
                            up += 1;
                            if !matches!(tokens[i].tok, Tok::Dot | Tok::DotDot) {
                                break;
                            }
                        }
                        _ => break,
                    }
                }
                let mut parts = Vec::new();
                while let Tok::Ident(name) = &tokens[i].tok {
                    parts.push(name.clone());
                    if tokens[i + 1].tok == Tok::Dot {
                        i += 2;
                    } else {
                        i += 1;
                        break;
                    }
                }
                let prefix = if up == 0 { "./".to_string() } else { "../".repeat(up) };
                out.push(format!("{prefix}{}.moph", parts.join("/")));
            }
            _ => {}
        }
    }
    Ok(out)
}

/// "./a/../b/./c.moph" → "b/c.moph"。根より上へ出る ".." は残す ("../orbit.moph")
pub fn normalize(path: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    for c in path.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => match parts.last() {
                Some(last) if last != ".." => {
                    parts.pop();
                }
                _ => parts.push("..".into()),
            },
            other => parts.push(other.as_os_str().to_string_lossy().into_owned()),
        }
    }
    parts.join("/")
}

pub fn root_of(path: &str) -> PathBuf {
    Path::new(path).parent().map(Path::to_path_buf).unwrap_or_default()
}
