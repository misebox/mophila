use std::fmt;

/// エラーの種別。ここに並んでいるものが全部で、文字列では書けない (綴りを間違えると通らない)。
/// 表示は "NameError.UndefinedAttribute" の形
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 構文として読めない
    UnexpectedToken,
    /// 色の桁数、Duration の形式など
    InvalidLiteral,
    UndefinedVariable,
    UndefinedAttribute,
    /// let を書かずに初めて代入した
    AssignWithoutLet,
    /// 組み込みの名前を宣言し直した
    Reserved,
    /// 演算子の左右
    OperandType,
    /// 属性に入れる値
    AttributeType,
    /// 引数
    ArgumentType,
    /// 引数や列の数
    ArityMismatch,
    /// place できない型
    NotPlaceable,
    /// 時刻の単位が混在、相対時刻なのに duration が無い
    DurationRequired,
    /// 型は合うが値が範囲外
    OutOfRange,
    DivisionByZero,
    FontNotFound,
    AudioUnreadable,
    ShaderCompile,
    ShaderUnavailable,
}

pub const KINDS: &[Kind] = &[
    Kind::UnexpectedToken,
    Kind::InvalidLiteral,
    Kind::UndefinedVariable,
    Kind::UndefinedAttribute,
    Kind::AssignWithoutLet,
    Kind::Reserved,
    Kind::OperandType,
    Kind::AttributeType,
    Kind::ArgumentType,
    Kind::ArityMismatch,
    Kind::NotPlaceable,
    Kind::DurationRequired,
    Kind::OutOfRange,
    Kind::DivisionByZero,
    Kind::FontNotFound,
    Kind::AudioUnreadable,
    Kind::ShaderCompile,
    Kind::ShaderUnavailable,
];

impl Kind {
    /// 仕様書とドキュメントに出す短い説明
    pub fn doc(self) -> &'static str {
        match self {
            Kind::UnexpectedToken => "構文として読めない",
            Kind::InvalidLiteral => "色の桁数、Duration の形式など、書き方が決まっている値が合わない",
            Kind::UndefinedVariable => "その名前が見つからない",
            Kind::UndefinedAttribute => "その型にその属性やメソッドが無い",
            Kind::AssignWithoutLet => "let を書かずに初めて代入した",
            Kind::Reserved => "組み込みの型や関数の名前を宣言し直した",
            Kind::OperandType => "演算子の左右の型が合わない",
            Kind::AttributeType => "属性に入れる値の型が合わない",
            Kind::ArgumentType => "引数の型が合わない",
            Kind::ArityMismatch => "引数や列の数が合わない",
            Kind::NotPlaceable => "place できない型を置いた",
            Kind::DurationRequired => "時刻の単位が混ざっている、または 0..1 で書いたのに duration が無い",
            Kind::OutOfRange => "型は合うが値が範囲外",
            Kind::DivisionByZero => "0 で割った",
            Kind::FontNotFound => "その名前のフォントが見つからない",
            Kind::AudioUnreadable => "音声ファイルを読めない",
            Kind::ShaderCompile => "Shader を WGSL に変換できない",
            Kind::ShaderUnavailable => "GPU が使えないので Shader を走らせられない",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::UnexpectedToken => "SyntaxError.UnexpectedToken",
            Kind::InvalidLiteral => "SyntaxError.InvalidLiteral",
            Kind::UndefinedVariable => "NameError.UndefinedVariable",
            Kind::UndefinedAttribute => "NameError.UndefinedAttribute",
            Kind::AssignWithoutLet => "NameError.AssignWithoutLet",
            Kind::Reserved => "NameError.Reserved",
            Kind::OperandType => "TypeError.OperandType",
            Kind::AttributeType => "TypeError.AttributeType",
            Kind::ArgumentType => "TypeError.ArgumentType",
            Kind::ArityMismatch => "TypeError.ArityMismatch",
            Kind::NotPlaceable => "TypeError.NotPlaceable",
            Kind::DurationRequired => "ValueError.DurationRequired",
            Kind::OutOfRange => "ValueError.OutOfRange",
            Kind::DivisionByZero => "RuntimeError.DivisionByZero",
            Kind::FontNotFound => "RuntimeError.FontNotFound",
            Kind::AudioUnreadable => "RuntimeError.AudioUnreadable",
            Kind::ShaderCompile => "RuntimeError.ShaderCompile",
            Kind::ShaderUnavailable => "RuntimeError.ShaderUnavailable",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// 種別.細目: message の形で表示するエラー
#[derive(Debug)]
pub struct MophError {
    pub kind: Kind,
    pub message: String,
}

impl MophError {
    pub fn new(kind: Kind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into() }
    }
}

impl fmt::Display for MophError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for MophError {}

pub type Result<T> = std::result::Result<T, MophError>;

pub fn err<T>(kind: Kind, message: impl Into<String>) -> Result<T> {
    Err(MophError::new(kind, message))
}

#[cfg(test)]
mod tests {
    /// 仕様書のエラーの表に、全部の種別が載っているか
    #[test]
    fn every_kind_is_in_the_spec() {
        let spec = include_str!("../../docs/mophila-spec.md");
        for kind in super::KINDS {
            let (_, detail) = kind.name().split_once('.').expect("種別.細目");
            assert!(spec.contains(detail), "{} が docs/mophila-spec.md に書かれていない", kind.name());
        }
    }
}
