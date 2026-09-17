" mophila (.moph)
" 語の一覧は src/lang/lexer.rs (キーワード) と mophila doc の types (型名) に合わせる

if exists("b:current_syntax")
  finish
endif

syn case match

syn keyword mophKeyword let func return if else for in import export as type record struct method private alias context motion output
syn keyword mophOperatorWord and or not
syn keyword mophBoolean true false
syn keyword mophBuiltin log type_of

syn keyword mophType Number Duration Bool String Symbol Tuple List Dict Range Func Vector Pos
syn keyword mophType Circle Ellipse Rect Line Polygon Path TextArea Color Gradient Shader View Timeline Motion
syn keyword mophType Image Audio Subtitle Anchor Align Easing StrokeCap StrokeJoin Blend GradientKind
syn keyword mophType Shape Placeable Paint Module Type Nothing

" import { a, b } from mod の from だけ。Line(from = ...) の from は属性なので色を変えない
syn match mophFrom "\%(^\s*import\>.*\)\@200<=\<from\>"

" 数 → Duration の順に書く。同じ位置で当たったときは後に書いた方が勝つ
syn match mophNumber "\<\d\+\%(\.\d\+\)\=\%([eE][-+]\=\d\+\)\=%\="
syn match mophDuration "\<\d\+\%(\.\d\+\)\=\%(ms\|s\|m\|h\)\>"
syn match mophDuration "\<\d\+:\d\d\%(:\d\d\)\=\%(\.\d\+\)\=\>"

syn match mophColor "#\x\{8}\>\|#\x\{6}\>\|#\x\{4}\>\|#\x\{3}\>"
syn match mophSymbol ":\a\w*"
syn match mophFunction "\<\h\w*\ze("
syn match mophOperator "->\|\.\.\.\|\.\.=\|\.\.\|[-+*/%^]\|[=<>!]=\|[<>=]"

syn match mophEscape "\\." contained
syn region mophString start=+"+ skip=+\\.+ end=+"+ contains=mophEscape

" # のあとが空白か行末ならコメント。#e04040 のような色リテラルとはここで分かれる
syn match mophComment "#\%(\s.*\)\=$" contains=@Spell
" ## で始まる行はドキュメントコメント (mophila doc と site が読む)
syn match mophDocComment "^\s*##\%(\s.*\)\=$" contains=mophDocTag,@Spell
syn match mophDocTag "@\%(category\|param\|returns\|example\)\>" contained

hi def link mophKeyword Keyword
hi def link mophFrom Keyword
hi def link mophOperatorWord Keyword
hi def link mophOperator Operator
hi def link mophBoolean Boolean
hi def link mophBuiltin Special
hi def link mophType Type
hi def link mophNumber Number
hi def link mophDuration Number
hi def link mophColor Constant
hi def link mophSymbol Constant
hi def link mophFunction Function
hi def link mophString String
hi def link mophEscape SpecialChar
hi def link mophComment Comment
hi def link mophDocComment SpecialComment
hi def link mophDocTag Identifier

let b:current_syntax = "mophila"
