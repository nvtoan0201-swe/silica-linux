use std::path::Path;
use std::sync::LazyLock;

use super::grammar::{Grammar, Pattern, TokenKind as K, pat, pat_group};

/// Languages the editor can highlight, detected from the file name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Language {
    Swift,
    JavaScript,
    TypeScript,
    Json,
    Python,
    Ruby,
    Go,
    Rust,
    C,
    Cpp,
    ObjectiveC,
    Java,
    Kotlin,
    CSharp,
    Php,
    Shell,
    Yaml,
    Toml,
    Html,
    Xml,
    Css,
    Markdown,
    Sql,
    Dockerfile,
    Makefile,
    Diff,
}

impl Language {
    pub fn for_path(path: &Path) -> Option<Language> {
        let name = path.file_name()?.to_string_lossy().to_lowercase();
        match name.as_str() {
            "dockerfile" | "containerfile" => return Some(Language::Dockerfile),
            "makefile" | "gnumakefile" => return Some(Language::Makefile),
            "gemfile" | "rakefile" | "podfile" | "fastfile" | "brewfile" => {
                return Some(Language::Ruby);
            }
            ".zshrc" | ".bashrc" | ".bash_profile" | ".zprofile" | ".profile" | ".envrc" => {
                return Some(Language::Shell);
            }
            ".gitignore" | ".gitattributes" | ".editorconfig" | ".npmrc" | ".env" => {
                return Some(Language::Toml);
            }
            _ => {}
        }
        if name.starts_with("dockerfile.") {
            return Some(Language::Dockerfile);
        }
        let ext = match name.rfind('.') {
            Some(dot) if dot > 0 => &name[dot + 1..],
            _ => return None,
        };
        Self::for_extension(ext)
    }

    fn for_extension(ext: &str) -> Option<Language> {
        Some(match ext {
            "swift" | "swiftinterface" => Language::Swift,
            "js" | "mjs" | "cjs" | "jsx" => Language::JavaScript,
            "ts" | "mts" | "cts" | "tsx" => Language::TypeScript,
            "json" | "jsonc" | "json5" | "xcproj" | "babelrc" | "eslintrc" | "webmanifest" => {
                Language::Json
            }
            "py" | "pyi" | "pyw" => Language::Python,
            "rb" | "rake" | "gemspec" | "podspec" => Language::Ruby,
            "go" => Language::Go,
            "rs" => Language::Rust,
            "c" | "h" => Language::C,
            "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" | "ipp" => Language::Cpp,
            "m" | "mm" => Language::ObjectiveC,
            "java" => Language::Java,
            "kt" | "kts" => Language::Kotlin,
            "cs" => Language::CSharp,
            "php" => Language::Php,
            "sh" | "bash" | "zsh" | "fish" | "command" => Language::Shell,
            "yml" | "yaml" => Language::Yaml,
            "toml" | "ini" | "cfg" | "conf" | "properties" | "xcconfig" | "env" => Language::Toml,
            "html" | "htm" | "xhtml" | "vue" | "svelte" => Language::Html,
            "xml" | "plist" | "svg" | "xib" | "storyboard" | "entitlements" | "xcscheme" | "xsd"
            | "rss" | "atom" => Language::Xml,
            "css" | "scss" | "sass" | "less" => Language::Css,
            "md" | "markdown" | "mdx" | "mdown" => Language::Markdown,
            "sql" => Language::Sql,
            "mk" => Language::Makefile,
            "diff" | "patch" | "rej" => Language::Diff,
            _ => return None,
        })
    }

    /// The language named on a Markdown code fence, e.g. ```swift or ```js.
    pub fn for_fence(name: &str) -> Option<Language> {
        let name = name.trim().to_lowercase();
        match name.as_str() {
            "" => None,
            "dockerfile" | "docker" => Some(Language::Dockerfile),
            "makefile" | "make" => Some(Language::Makefile),
            _ => {
                let alias = match name.as_str() {
                    "javascript" => "js",
                    "typescript" => "ts",
                    "python" => "py",
                    "ruby" => "rb",
                    "rust" => "rs",
                    "objc" | "objective-c" | "objectivec" => "m",
                    "c++" => "cpp",
                    "csharp" | "c#" => "cs",
                    "kotlin" => "kt",
                    "shell" | "console" => "sh",
                    "yaml" => "yml",
                    "markdown" => "md",
                    "golang" => "go",
                    other => other,
                };
                Self::for_extension(alias)
            }
        }
    }

    /// Shown in the status bar.
    pub fn display_name(self) -> &'static str {
        match self {
            Language::Swift => "Swift",
            Language::JavaScript => "JavaScript",
            Language::TypeScript => "TypeScript",
            Language::Json => "JSON",
            Language::Python => "Python",
            Language::Ruby => "Ruby",
            Language::Go => "Go",
            Language::Rust => "Rust",
            Language::C => "C",
            Language::Cpp => "C++",
            Language::ObjectiveC => "Objective-C",
            Language::Java => "Java",
            Language::Kotlin => "Kotlin",
            Language::CSharp => "C#",
            Language::Php => "PHP",
            Language::Shell => "Shell",
            Language::Yaml => "YAML",
            Language::Toml => "TOML",
            Language::Html => "HTML",
            Language::Xml => "XML",
            Language::Css => "CSS",
            Language::Markdown => "Markdown",
            Language::Sql => "SQL",
            Language::Dockerfile => "Dockerfile",
            Language::Makefile => "Makefile",
            Language::Diff => "Diff",
        }
    }

    /// Starts a line comment, for Toggle Comment; `None` where only block comments exist.
    pub fn line_comment(self) -> Option<&'static str> {
        use Language::*;
        match self {
            Swift | JavaScript | TypeScript | Json | Go | Rust | C | Cpp | ObjectiveC | Java
            | Kotlin | CSharp | Php => Some("//"),
            Python | Ruby | Shell | Yaml | Toml | Dockerfile | Makefile => Some("#"),
            Sql => Some("--"),
            Html | Xml | Css | Markdown | Diff => None,
        }
    }

    /// Wraps a commented line in languages without line comments.
    pub fn block_comment(self) -> Option<(&'static str, &'static str)> {
        match self {
            Language::Css => Some(("/*", "*/")),
            Language::Html | Language::Xml | Language::Markdown => Some(("<!--", "-->")),
            _ => None,
        }
    }

    /// Lines ending with `:` open a block (Python's `def f():`, YAML mappings).
    pub fn opens_block_with_colon(self) -> bool {
        matches!(self, Language::Python | Language::Yaml)
    }

    pub fn grammar(self) -> &'static Grammar {
        use Language::*;
        match self {
            Swift => &SWIFT,
            JavaScript | TypeScript => &JAVASCRIPT,
            Json => &JSON,
            Python => &PYTHON,
            Ruby => &RUBY,
            Go => &GO,
            Rust => &RUST,
            C | Cpp | ObjectiveC => &C_FAMILY,
            Java => &JAVA,
            Kotlin => &KOTLIN,
            CSharp => &CSHARP,
            Php => &PHP,
            Shell => &SHELL,
            Yaml => &YAML,
            Toml => &TOML,
            Html | Xml => &MARKUP,
            Css => &CSS,
            Markdown => &MARKDOWN,
            Sql => &SQL,
            Dockerfile => &DOCKERFILE,
            Makefile => &MAKEFILE,
            Diff => &DIFF,
        }
    }
}

// MARK: - Shared patterns

const LINE_COMMENT: &str = r#"//.*"#;
const BLOCK_COMMENT: &str = r#"/\*[\s\S]*?(?:\*/|\z)"#;
/// `#` comment that starts a line or follows whitespace (so `$#` / `a#b` aren't comments).
const HASH_COMMENT: &str = r#"(?:^|(?<=\s))#.*"#;
const DOUBLE_STRING: &str = r#""(?:\\.|[^"\\\n])*""#;
const SINGLE_STRING: &str = r#"'(?:\\.|[^'\\\n])*'"#;
const TRIPLE_DOUBLE: &str = r#""""[\s\S]*?(?:"""|\z)"#;
const TRIPLE_SINGLE: &str = r#"'''[\s\S]*?(?:'''|\z)"#;
const BACKTICK: &str = r#"`(?:\\.|[^`\\])*`"#;
const NUMBER: &str = r#"\b(?:0[xX][0-9a-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)[a-zA-Z]*\b"#;
const CAPITALIZED: &str = r#"\b[A-Z][A-Za-z0-9_]*\b"#;
const CALL: &str = r#"\b([A-Za-z_]\w*)\s*(?=\()"#;

fn words(list: &str) -> String {
    format!("\\b(?:{})\\b", list.split_whitespace().collect::<Vec<_>>().join("|"))
}

fn declared(after: &str) -> String {
    format!("\\b(?:{after})\\s+([A-Za-z_]\\w*)")
}

fn grammar(rules: Vec<Pattern>, spans: Vec<Pattern>) -> Grammar {
    Grammar::new(rules, spans)
}

// MARK: - Grammars

static SWIFT: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat_group(K::Function, CALL, 1),
            pat_group(K::Function, declared("func"), 1),
            pat(K::Type, CAPITALIZED),
            pat(K::Number, NUMBER),
            pat(K::Attribute, r#"@\w+"#),
            pat(
                K::Keyword,
                words(
                    "associatedtype class deinit enum extension fileprivate func import init inout internal let \
                     open operator private precedencegroup protocol public rethrows static struct subscript typealias \
                     var break case catch continue default defer do else fallthrough for guard if in repeat return \
                     throw switch where while as Any await async false is nil self Self super throws true try actor \
                     some any nonisolated isolated consuming borrowing mutating nonmutating override final lazy weak \
                     unowned required convenience dynamic indirect optional get set willSet didSet macro package \
                     sending each",
                ),
            ),
            pat(K::Preprocessor, r#"#\w+"#),
        ],
        vec![
            pat(K::Comment, BLOCK_COMMENT),
            pat(K::Comment, LINE_COMMENT),
            pat(K::String, TRIPLE_DOUBLE),
            pat(K::String, DOUBLE_STRING),
        ],
    )
});

static JAVASCRIPT: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat_group(K::Function, CALL, 1),
            pat(K::Type, CAPITALIZED),
            pat(K::Number, NUMBER),
            pat(K::Attribute, r#"@\w+"#),
            pat(
                K::Keyword,
                words(
                    "break case catch class const continue debugger default delete do else export extends finally \
                     for function if import in instanceof let new return super switch this throw try typeof var void \
                     while with yield async await of static get set from as true false null undefined NaN Infinity \
                     interface type enum implements private protected public readonly abstract declare namespace \
                     module keyof infer is satisfies override any unknown never string number boolean bigint symbol \
                     object asserts accessor",
                ),
            ),
        ],
        vec![
            pat(K::Comment, BLOCK_COMMENT),
            pat(K::Comment, LINE_COMMENT),
            pat(K::String, BACKTICK),
            pat(K::String, DOUBLE_STRING),
            pat(K::String, SINGLE_STRING),
        ],
    )
});

static JSON: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat(K::Number, r#"-?\b\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b"#),
            pat(K::Keyword, words("true false null")),
        ],
        vec![
            pat(K::Comment, BLOCK_COMMENT),
            pat(K::Comment, LINE_COMMENT),
            pat(K::Property, r#""(?:\\.|[^"\\\n])*"(?=\s*:)"#),
            pat(K::String, DOUBLE_STRING),
        ],
    )
});

static PYTHON: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat_group(K::Function, CALL, 1),
            pat(K::Type, CAPITALIZED),
            pat(
                K::Type,
                words("int str float bool bytes list dict set tuple object type complex frozenset bytearray"),
            ),
            pat_group(K::Function, declared("def"), 1),
            pat_group(K::Type, declared("class"), 1),
            pat(K::Number, NUMBER),
            pat(K::Attribute, r#"^[ \t]*@[\w.]+"#),
            pat(
                K::Keyword,
                words(
                    "False None True and as assert async await break class continue def del elif else except \
                     finally for from global if import in is lambda nonlocal not or pass raise return try while \
                     with yield match case self cls",
                ),
            ),
        ],
        vec![
            pat(K::Comment, r#"#.*"#),
            pat(K::String, r#"(?:[rRbBuUfF]{1,2})?"""[\s\S]*?(?:"""|\z)"#),
            pat(K::String, r#"(?:[rRbBuUfF]{1,2})?'''[\s\S]*?(?:'''|\z)"#),
            pat(K::String, r#"(?:[rRbBuUfF]{1,2})?"(?:\\.|[^"\\\n])*""#),
            pat(K::String, r#"(?:[rRbBuUfF]{1,2})?'(?:\\.|[^'\\\n])*'"#),
        ],
    )
});

static RUBY: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat(K::Type, CAPITALIZED),
            pat_group(K::Function, declared("def"), 1),
            pat(K::Number, NUMBER),
            pat(K::Attribute, r#"(?<![:\w]):[A-Za-z_]\w*[?!]?"#),
            pat(K::Variable, r#"@{1,2}\w+|\$\w+"#),
            pat(
                K::Keyword,
                words(
                    "alias and begin break case class def do else elsif end ensure false for if in module next nil \
                     not or redo rescue retry return self super then true undef unless until when while yield \
                     require require_relative attr_accessor attr_reader attr_writer private protected public lambda \
                     proc puts raise",
                ),
            ),
        ],
        vec![
            pat(K::Comment, r#"^=begin[\s\S]*?(?:^=end|\z)"#),
            pat(K::Comment, HASH_COMMENT),
            pat(K::String, DOUBLE_STRING),
            pat(K::String, SINGLE_STRING),
        ],
    )
});

static GO: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat(K::Type, CAPITALIZED),
            pat(
                K::Type,
                words(
                    "bool byte complex64 complex128 error float32 float64 int int8 int16 int32 int64 rune string \
                     uint uint8 uint16 uint32 uint64 uintptr any comparable",
                ),
            ),
            pat_group(K::Function, CALL, 1),
            pat(K::Number, NUMBER),
            pat(
                K::Keyword,
                words(
                    "break case chan const continue default defer else fallthrough for func go goto if import \
                     interface map package range return select struct switch type var true false nil iota",
                ),
            ),
        ],
        vec![
            pat(K::Comment, BLOCK_COMMENT),
            pat(K::Comment, LINE_COMMENT),
            pat(K::String, BACKTICK),
            pat(K::String, DOUBLE_STRING),
            pat(K::String, r#"'(?:\\.|[^'\\\n])+'"#),
        ],
    )
});

static RUST: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat_group(K::Function, CALL, 1),
            pat(K::Type, CAPITALIZED),
            pat(
                K::Type,
                words("i8 i16 i32 i64 i128 isize u8 u16 u32 u64 u128 usize f32 f64 bool char str"),
            ),
            pat(K::Number, NUMBER),
            pat(K::Preprocessor, r#"\b[a-z_]\w*!(?=\s*[(\[{])"#),
            pat(K::Attribute, r#"#!?\[[^\]\n]*\]"#),
            pat(K::Attribute, r#"'[a-zA-Z_]\w*\b(?!')"#),
            pat(
                K::Keyword,
                words(
                    "as async await break const continue crate dyn else enum extern false fn for if impl in let \
                     loop match mod move mut pub ref return self Self static struct super trait true type unsafe \
                     use where while union",
                ),
            ),
        ],
        vec![
            pat(K::Comment, BLOCK_COMMENT),
            pat(K::Comment, LINE_COMMENT),
            pat(K::String, r#"b?"(?:\\.|[^"\\])*""#),
            pat(K::String, r#"b?'(?:\\.|[^'\\\n])'"#),
        ],
    )
});

static C_FAMILY: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat_group(K::Function, CALL, 1),
            pat(K::Type, CAPITALIZED),
            pat(K::Number, NUMBER),
            pat(K::Keyword, r#"@\w+"#),
            pat(
                K::Keyword,
                words(
                    "auto break case char const continue default do double else enum extern float for goto if \
                     inline int long register restrict return short signed sizeof static struct switch typedef \
                     union unsigned void volatile while bool true false NULL nullptr class namespace template \
                     typename public private protected virtual override final friend operator new delete this \
                     throw try catch using constexpr consteval constinit noexcept static_cast dynamic_cast \
                     reinterpret_cast const_cast decltype explicit mutable concept requires co_await co_return \
                     co_yield thread_local alignas alignof wchar_t size_t self super id nil YES NO instancetype \
                     BOOL SEL IMP nonatomic atomic strong weak copy assign readonly readwrite nullable nonnull",
                ),
            ),
            pat(K::Preprocessor, r#"^[ \t]*#[ \t]*\w+"#),
            pat_group(K::String, r#"^[ \t]*#[ \t]*(?:include|import)[ \t]*(<[^>\n]*>)"#, 1),
        ],
        vec![
            pat(K::Comment, BLOCK_COMMENT),
            pat(K::Comment, LINE_COMMENT),
            pat(K::String, r#"@?"(?:\\.|[^"\\\n])*""#),
            pat(K::String, r#"'(?:\\.|[^'\\\n])+'"#),
        ],
    )
});

static JAVA: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat_group(K::Function, CALL, 1),
            pat(K::Type, CAPITALIZED),
            pat(K::Number, NUMBER),
            pat(K::Attribute, r#"@\w+"#),
            pat(
                K::Keyword,
                words(
                    "abstract assert boolean break byte case catch char class const continue default do double \
                     else enum extends final finally float for goto if implements import instanceof int interface \
                     long native new package private protected public return short static strictfp super switch \
                     synchronized this throw throws transient try void volatile while true false null var record \
                     sealed permits yield",
                ),
            ),
        ],
        vec![
            pat(K::Comment, BLOCK_COMMENT),
            pat(K::Comment, LINE_COMMENT),
            pat(K::String, TRIPLE_DOUBLE),
            pat(K::String, DOUBLE_STRING),
            pat(K::String, r#"'(?:\\.|[^'\\\n])+'"#),
        ],
    )
});

static KOTLIN: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat_group(K::Function, CALL, 1),
            pat_group(K::Function, declared("fun"), 1),
            pat(K::Type, CAPITALIZED),
            pat(K::Number, NUMBER),
            pat(K::Attribute, r#"@\w+"#),
            pat(
                K::Keyword,
                words(
                    "as break class continue do else false for fun if in interface is null object package return \
                     super this throw true try typealias typeof val var when while by catch constructor finally get \
                     import init set where actual abstract annotation companion const crossinline data enum expect \
                     external final infix inline inner internal lateinit noinline open operator out override \
                     private protected public reified sealed suspend tailrec vararg it",
                ),
            ),
        ],
        vec![
            pat(K::Comment, BLOCK_COMMENT),
            pat(K::Comment, LINE_COMMENT),
            pat(K::String, TRIPLE_DOUBLE),
            pat(K::String, DOUBLE_STRING),
            pat(K::String, r#"'(?:\\.|[^'\\\n])+'"#),
        ],
    )
});

static CSHARP: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat_group(K::Function, CALL, 1),
            pat(K::Type, CAPITALIZED),
            pat(K::Number, NUMBER),
            pat(K::Attribute, r#"^[ \t]*\[[A-Z][^\]\n]*\]"#),
            pat(
                K::Keyword,
                words(
                    "abstract as base bool break byte case catch char checked class const continue decimal default \
                     delegate do double else enum event explicit extern false finally fixed float for foreach goto \
                     if implicit in int interface internal is lock long namespace new null object operator out \
                     override params private protected public readonly ref return sbyte sealed short sizeof \
                     stackalloc static string struct switch this throw true try typeof uint ulong unchecked unsafe \
                     ushort using virtual void volatile while var async await dynamic get set value yield record \
                     init required global nameof when with not and or",
                ),
            ),
            pat(K::Preprocessor, r#"^[ \t]*#[ \t]*\w+"#),
        ],
        vec![
            pat(K::Comment, BLOCK_COMMENT),
            pat(K::Comment, LINE_COMMENT),
            pat(K::String, r#"@"(?:""|[^"])*""#),
            pat(K::String, r#"\$?"(?:\\.|[^"\\\n])*""#),
            pat(K::String, r#"'(?:\\.|[^'\\\n])+'"#),
        ],
    )
});

static PHP: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat_group(K::Function, CALL, 1),
            pat(K::Type, CAPITALIZED),
            pat(K::Number, NUMBER),
            pat(K::Variable, r#"\$\w+"#),
            pat(K::Preprocessor, r#"<\?(?:php|=)?|\?>"#),
            pat(
                K::Keyword,
                words(
                    "abstract and array as break callable case catch class clone const continue declare default do \
                     echo else elseif empty enddeclare endfor endforeach endif endswitch endwhile extends final \
                     finally fn for foreach function global goto if implements include include_once instanceof \
                     insteadof interface isset list match namespace new or print private protected public readonly \
                     require require_once return static switch throw trait try unset use var while xor yield true \
                     false null self parent enum",
                ),
            ),
        ],
        vec![
            pat(K::Comment, BLOCK_COMMENT),
            pat(K::Comment, LINE_COMMENT),
            pat(K::Comment, HASH_COMMENT),
            pat(K::String, DOUBLE_STRING),
            pat(K::String, SINGLE_STRING),
        ],
    )
});

static SHELL: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat_group(K::Function, r#"^[ \t]*(?:function[ \t]+)?([A-Za-z_][\w-]*)[ \t]*\(\)"#, 1),
            pat(K::Number, r#"\b\d+\b"#),
            pat(K::Variable, r#"\$\{[^}\n]*\}|\$[A-Za-z_]\w*|\$[0-9@#?*$!-]"#),
            pat(
                K::Keyword,
                words(
                    "if then else elif fi for while until do done case esac in function return local export source \
                     alias unset declare readonly select time break continue exit eval exec shift trap set test \
                     echo printf cd read",
                ),
            ),
        ],
        vec![
            pat(K::Comment, HASH_COMMENT),
            pat(K::String, r#""(?:\\.|[^"\\])*""#),
            pat(K::String, r#"'[^']*'"#),
        ],
    )
});

static YAML: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat(K::Number, r#"(?<![\w.-])-?\d+(?:\.\d+)?(?![\w.-])"#),
            pat(K::Keyword, r#"(?i)(?<![\w-])(?:true|false|yes|no|on|off|null)(?![\w-])"#),
            pat(K::Attribute, r#"[&*][\w-]+"#),
            pat(K::Preprocessor, r#"^(?:---|\.\.\.)[ \t]*$"#),
            pat_group(
                K::Property,
                r#"^[ \t]*(?:-[ \t]+)?([\w$.\-/]+(?:[ \t]+[\w$.\-/]+)*)[ \t]*:(?=[ \t]|$)"#,
                1,
            ),
        ],
        vec![
            pat(K::Comment, HASH_COMMENT),
            pat(K::String, DOUBLE_STRING),
            pat(K::String, r#"'(?:''|[^'\n])*'"#),
        ],
    )
});

static TOML: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat(K::Number, r#"(?<![\w.-])[+-]?\d[\d_]*(?:\.\d+)?(?![\w.-])"#),
            pat(K::Keyword, words("true false")),
            pat_group(K::Property, r#"^[ \t]*([\w.\-]+)[ \t]*="#, 1),
            pat(K::Type, r#"^[ \t]*\[\[?[^\]\n]*\]\]?"#),
        ],
        vec![
            pat(K::Comment, r#"(?:(?:^|(?<=\s))#.*|^[ \t]*;.*)"#),
            pat(K::String, TRIPLE_DOUBLE),
            pat(K::String, TRIPLE_SINGLE),
            pat(K::String, DOUBLE_STRING),
            pat(K::String, r#"'[^'\n]*'"#),
        ],
    )
});

static MARKUP: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat_group(K::Tag, r#"</?([A-Za-z][\w:.\-]*)"#, 1),
            pat_group(K::Attribute, r#"(?<=\s)([A-Za-z_:@][\w:.\-]*)(?=\s*=)"#, 1),
            pat_group(K::String, r#"=\s*("[^"]*"|'[^']*')"#, 1),
            pat(K::Number, r#"&(?:\w+|#\d+|#x[0-9a-fA-F]+);"#),
            pat(K::Preprocessor, r#"<![A-Za-z][^>]*>|<\?[\s\S]*?\?>"#),
        ],
        vec![
            pat(K::Comment, r#"<!--[\s\S]*?(?:-->|\z)"#),
            pat(K::String, r#"<!\[CDATA\[[\s\S]*?(?:\]\]>|\z)"#),
        ],
    )
});

static CSS: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat(K::Type, r#"(?<![\w-])\.[A-Za-z_-][\w-]*"#),
            pat_group(K::Function, r#"\b([\w-]+)\("#, 1),
            pat_group(K::Property, r#"^[ \t]*([A-Za-z-]+)[ \t]*:(?![^;\n]*\{)"#, 1),
            pat_group(K::Property, r#"[{;][ \t]*([A-Za-z-]+)[ \t]*:"#, 1),
            pat(
                K::Number,
                r#"(?<![\w#.-])-?\d*\.?\d+(?:px|em|rem|vh|vw|vmin|vmax|ch|ex|pt|pc|cm|mm|in|ms|s|deg|rad|turn|fr|dpi|dppx|%)?"#,
            ),
            pat(K::Number, r#"#[0-9a-fA-F]{3,8}\b"#),
            pat(K::Variable, r#"--[\w-]+|\$[\w-]+"#),
            pat(K::Keyword, r#"@[\w-]+|!important"#),
        ],
        vec![
            pat(K::Comment, BLOCK_COMMENT),
            pat(K::String, DOUBLE_STRING),
            pat(K::String, SINGLE_STRING),
        ],
    )
});

static MARKDOWN: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat(K::Heading, r#"^#{1,6}[ \t].*$"#),
            pat(K::Comment, r#"^[ \t]*>.*$"#),
            pat(K::Comment, r#"^[ \t]*(?:-{3,}|\*{3,}|_{3,})[ \t]*$"#),
            pat(K::Keyword, r#"^[ \t]*(?:[-*+]|\d+[.)])(?=[ \t])"#),
            pat(K::Attribute, r#"(\*\*|__)(?=\S)[^\n]*?\S\1"#),
            pat(K::Link, r#"!?\[[^\]\n]*\]\([^)\n]*\)|<https?://[^>\s]+>"#),
        ],
        vec![
            pat(K::String, r#"^[ \t]*```[\s\S]*?(?:^[ \t]*```|\z)"#),
            pat(K::String, r#"^[ \t]*~~~[\s\S]*?(?:^[ \t]*~~~|\z)"#),
            pat(K::Comment, r#"<!--[\s\S]*?(?:-->|\z)"#),
            pat(K::String, r#"`[^`\n]+`"#),
        ],
    )
});

static SQL: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat_group(K::Function, CALL, 1),
            pat(K::Number, NUMBER),
            pat(
                K::Keyword,
                format!(
                    "(?i){}",
                    words(
                        "select from where insert into values update set delete create table alter drop index view \
                         join inner left right outer full cross on as and or not null is in exists between like ilike \
                         group by order having limit offset union all distinct primary key foreign references default \
                         unique check constraint case when then else end begin commit rollback transaction returning \
                         with if replace asc desc true false integer int bigint smallint text varchar char boolean \
                         date timestamp numeric real serial cascade trigger function procedure return returns",
                    )
                ),
            ),
        ],
        vec![
            pat(K::Comment, r#"--.*"#),
            pat(K::Comment, BLOCK_COMMENT),
            pat(K::String, r#"'(?:''|[^'])*'"#),
            pat(K::Property, r#""[^"\n]*"|`[^`\n]*`"#),
        ],
    )
});

static DOCKERFILE: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat(K::Variable, r#"\$\{[^}\n]*\}|\$[A-Za-z_]\w*"#),
            pat(
                K::Keyword,
                r#"(?i)^[ \t]*(?:FROM|RUN|CMD|LABEL|MAINTAINER|EXPOSE|ENV|ADD|COPY|ENTRYPOINT|VOLUME|USER|WORKDIR|ARG|ONBUILD|STOPSIGNAL|HEALTHCHECK|SHELL)\b|\bAS\b"#,
            ),
        ],
        vec![
            pat(K::Comment, r#"^[ \t]*#.*"#),
            pat(K::String, DOUBLE_STRING),
            pat(K::String, SINGLE_STRING),
        ],
    )
});

static MAKEFILE: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat_group(K::Function, r#"^([^\s:#=][^:#=\n]*?)[ \t]*:(?!=)"#, 1),
            pat_group(K::Property, r#"^[ \t]*([\w.]+)[ \t]*(?:[:+?!]?=)"#, 1),
            pat(K::Variable, r#"\$\([^)\n]*\)|\$\{[^}\n]*\}|\$[@<^?*%+]"#),
            pat(
                K::Keyword,
                r#"^[ \t]*-?(?:include|ifeq|ifneq|ifdef|ifndef|else|endif|define|endef|export|override)\b"#,
            ),
        ],
        vec![
            pat(K::Comment, HASH_COMMENT),
            pat(K::String, DOUBLE_STRING),
            pat(K::String, SINGLE_STRING),
        ],
    )
});

/// Unified diffs as `git diff` and `git format-patch` write them, plus the older
/// context and normal formats. A line is classified by how it starts, so `+` and `-`
/// rules come first and the headers that also start with them override.
static DIFF: LazyLock<Grammar> = LazyLock::new(|| {
    grammar(
        vec![
            pat(K::Inserted, r#"^(?:\+|>(?= |$)).*"#),
            pat(K::Deleted, r#"^(?:-|<(?= |$)).*"#),
            // A changed line in a context diff.
            pat(K::Type, r#"^!(?= |$).*"#),
            // The bar of a diffstat line: ` src/main.rs | 12 +++++-----`.
            pat_group(K::Inserted, r#"^ \S.*\|[ \t]+\d+ (\++)-*$"#, 1),
            pat_group(K::Deleted, r#"^ \S.*\|[ \t]+\d+ \+*(-+)$"#, 1),
            // File headers. `--- a` only counts with `+++ b` (or, in a context diff, `*** a`)
            // beside it, so a removed `-- comment` line stays a removed line.
            pat(K::Type, r#"^(?:diff -|Index: |={10,}$).*"#),
            pat(K::Type, r#"^--- .*\r?\n\+\+\+ .*"#),
            pat(K::Type, r#"^\*\*\* .*(?:\r?\n--- .*)?"#),
            pat(
                K::Comment,
                r#"^(?:index [0-9a-f,]+\.\.[0-9a-f]+|old mode |new mode |deleted file mode |new file mode |similarity index |dissimilarity index |rename from |rename to |copy from |copy to |Binary files |GIT binary patch|Only in |\\ ).*"#,
            ),
            // The `---` before a diffstat or between the halves of a normal diff.
            pat(
                K::Comment,
                r#"^---(?=\r?\n(?:>(?: |$)| \S.*\| +(?:\d+(?: [+-]+)?|Bin .*)$| \d+ files? changed|\r?\n))"#,
            ),
            // The mail signature (`-- ` above git's version) and a context diff's separator.
            pat(K::Comment, r#"^(?:-- (?=\r?\n\d)|\*{15}$)"#),
            // Hunk ranges: `@@ -1,4 +1,6 @@`, then the context and normal forms.
            pat(K::Link, r#"^(@@+) [-+\d, ]+ \1"#),
            pat(K::Link, r#"^(?:\*\*\* \d+(?:,\d+)? \*\*\*\*|--- \d+(?:,\d+)? ----)$"#),
            pat(K::Link, r#"^\d+(?:,\d+)?[acd]\d+(?:,\d+)?$"#),
            // A format-patch's mail header and commit message, through the `---` that ends them.
            pat(K::Comment, r#"^From [0-9a-f]{40} [\s\S]*?(?:^---$|\z)"#),
        ],
        vec![],
    )
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::grammar::TokenKind;

    const ALL: [Language; 26] = [
        Language::Swift,
        Language::JavaScript,
        Language::TypeScript,
        Language::Json,
        Language::Python,
        Language::Ruby,
        Language::Go,
        Language::Rust,
        Language::C,
        Language::Cpp,
        Language::ObjectiveC,
        Language::Java,
        Language::Kotlin,
        Language::CSharp,
        Language::Php,
        Language::Shell,
        Language::Yaml,
        Language::Toml,
        Language::Html,
        Language::Xml,
        Language::Css,
        Language::Markdown,
        Language::Sql,
        Language::Dockerfile,
        Language::Makefile,
        Language::Diff,
    ];

    #[test]
    fn every_grammar_compiles() {
        for language in ALL {
            language.grammar().highlight("let x = \"a\" // b\n");
        }
    }

    #[test]
    fn strings_win_over_comments_inside_them() {
        let spans = Language::Rust.grammar().highlight("let s = \"// not a comment\";");
        assert!(spans.iter().any(|s| s.kind == TokenKind::String && s.range == (8..26)));
        assert!(!spans.iter().any(|s| s.kind == TokenKind::Comment));
    }

    #[test]
    fn detects_by_name() {
        assert_eq!(Language::for_path(Path::new("/a/Makefile")), Some(Language::Makefile));
        assert_eq!(Language::for_path(Path::new("x.tsx")), Some(Language::TypeScript));
        assert_eq!(Language::for_fence("c++"), Some(Language::Cpp));
    }
}
