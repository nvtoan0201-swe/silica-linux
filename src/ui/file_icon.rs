//! Icon + tint for a file, colored like Xcode's project navigator: blue folders and
//! orange Swift files, each language's mark in its own color, and a hue per other file
//! family.

use std::path::Path;

use gpui::Hsla;

use crate::theme::hex;

/// A light-mode and a dark-mode shade.
#[derive(Clone, Copy)]
struct Pair(u32, u32);

impl Pair {
    fn color(self, dark: bool) -> Hsla {
        hex(if dark { self.1 } else { self.0 })
    }
}

const FOLDER: Pair = Pair(0x3B8EEA, 0x5AA5F5);
const SWIFT: Pair = Pair(0xF05138, 0xF4693F);
const BLUE: Pair = Pair(0x2F74C0, 0x5E9BEA);
const MINT: Pair = Pair(0x2E9C78, 0x5CC9A2);
const GREEN: Pair = Pair(0x3A9150, 0x63C27A);
const YELLOW: Pair = Pair(0xB88A00, 0xEAC04A);
const RED: Pair = Pair(0xCC3D3D, 0xF06C66);
const PURPLE: Pair = Pair(0x8A45C4, 0xB582EE);
const BROWN: Pair = Pair(0x976236, 0xC9976A);
const STEEL: Pair = Pair(0x5F7C9B, 0x92ADC9);
const NEUTRAL: Pair = Pair(0x8A8D94, 0xA2A5AC);

// Language marks, in (a shade of) the language's own color.
const RUST: Pair = Pair(0xB7410E, 0xE8835A);
const PYTHON: Pair = Pair(0x3776AB, 0x5D9BD6);
const JAVASCRIPT: Pair = Pair(0xC9A800, 0xF7DF1E);
const TYPESCRIPT: Pair = Pair(0x3178C6, 0x5A9BE6);
const GO: Pair = Pair(0x00ADD8, 0x3FCBEF);
const C: Pair = Pair(0x5C6BC0, 0x8C9EFF);
const CPP: Pair = Pair(0x00599C, 0x4B9BE0);
const DOTNET: Pair = Pair(0x512BD4, 0x9A7BF0);
const JAVA: Pair = Pair(0xE76F00, 0xF59A45);
const KOTLIN: Pair = Pair(0x7F52FF, 0xA88BFF);
const RUBY: Pair = Pair(0xCC342D, 0xF0605A);
const PHP: Pair = Pair(0x777BB4, 0x9EA2DC);
const HTML: Pair = Pair(0xE34F26, 0xF4774F);
const CSS: Pair = Pair(0x663399, 0xA77BDB);
const SASS: Pair = Pair(0xCC6699, 0xE88BB8);
const LESS: Pair = Pair(0x1D365D, 0x6A8FC7);
const MARKDOWN: Pair = Pair(0x4A4A4F, 0xC8C8CE);
const JSON: Pair = Pair(0xC26F0F, 0xF09A45);
const DOCKER: Pair = Pair(0x2496ED, 0x52B0F5);
const GIT: Pair = Pair(0xF05032, 0xF5775F);
const SHELL: Pair = Pair(0x3A9150, 0x63C27A);
const LUA: Pair = Pair(0x2C2D72, 0x7B7CE0);
const DART: Pair = Pair(0x0175C2, 0x3FA3E8);
const SCALA: Pair = Pair(0xDC322F, 0xF0605A);
const HASKELL: Pair = Pair(0x5D4F85, 0x9D8FCA);
const ELIXIR: Pair = Pair(0x4B275F, 0x9C6DB8);
const ZIG: Pair = Pair(0xD98A00, 0xF7A41D);
const VUE: Pair = Pair(0x2F9C6E, 0x4FC08D);
const SVELTE: Pair = Pair(0xFF3E00, 0xFF6A3D);
const REACT: Pair = Pair(0x0A9AC4, 0x61DAFB);
const YAML: Pair = Pair(0xCB171E, 0xF0605A);
const TOML: Pair = Pair(0x9C4121, 0xD9774F);
const NIX: Pair = Pair(0x5277C3, 0x7EA3EA);
const R: Pair = Pair(0x276DC3, 0x5A9BE6);
const JULIA: Pair = Pair(0x9558B2, 0xB98BD6);
const PERL: Pair = Pair(0x39457E, 0x7D8CD0);
const OCAML: Pair = Pair(0xEC6813, 0xF58E4A);
const ERLANG: Pair = Pair(0xA90533, 0xE44D6E);
const CLOJURE: Pair = Pair(0x5881D8, 0x82A4EC);
const CMAKE: Pair = Pair(0x064F8C, 0x4B8FD0);
const GRADLE: Pair = Pair(0x02303A, 0x5FB3C4);
const NODE: Pair = Pair(0x5FA04E, 0x7CC36A);
const NPM: Pair = Pair(0xCB3837, 0xF0605A);
const GRAPHQL: Pair = Pair(0xE10098, 0xF55BC0);
const SQL: Pair = Pair(0x4169E1, 0x6F8FF0);
const ASTRO: Pair = Pair(0xBC52EE, 0xD285F5);
const SOLIDITY: Pair = Pair(0x363636, 0xB0B0B8);
const TERRAFORM: Pair = Pair(0x7B42BC, 0xA97AE0);
const LATEX: Pair = Pair(0x008080, 0x3FBFBF);
const VIM: Pair = Pair(0x019733, 0x3FC266);

/// The icon name and its color.
pub fn style(path: &Path, is_dir: bool, expanded: bool, dark: bool) -> (&'static str, Hsla) {
    if is_dir {
        return (if expanded { "folder-open-fill" } else { "folder-fill" }, FOLDER.color(dark));
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let (icon, pair) = match name.as_str() {
        "package.swift" => ("swift", SWIFT),
        "cargo.toml" | "cargo.lock" => ("lang/rust", RUST),
        "package.json" | "package-lock.json" | ".npmrc" => ("lang/npm", NPM),
        "dockerfile" | "containerfile" | ".dockerignore" | "docker-compose.yml" | "docker-compose.yaml"
        | "compose.yml" | "compose.yaml" => ("lang/docker", DOCKER),
        "cmakelists.txt" => ("lang/cmake", CMAKE),
        "makefile" | "gnumakefile" | "justfile" => ("hammer", BROWN),
        "license" | "license.md" | "license.txt" | "copying" => ("badge-check", YELLOW),
        ".gitignore" | ".gitattributes" | ".gitmodules" | ".gitkeep" => ("lang/git", GIT),
        ".vimrc" | ".gvimrc" => ("lang/vim", VIM),
        ".bashrc" | ".zshrc" | ".profile" | ".bash_profile" | ".zprofile" => ("lang/gnubash", SHELL),
        "flake.nix" | "flake.lock" => ("lang/nixos", NIX),
        ".nvmrc" | ".node-version" => ("lang/nodedotjs", NODE),
        _ => {
            if name.ends_with(".gradle") || name.ends_with(".gradle.kts") {
                return ("lang/gradle", GRADLE.color(dark));
            }
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            match ext.as_str() {
                "swift" => ("swift", SWIFT),
                "rs" => ("lang/rust", RUST),
                "py" | "pyi" | "pyw" | "ipynb" => ("lang/python", PYTHON),
                "js" | "mjs" | "cjs" => ("lang/javascript", JAVASCRIPT),
                "ts" | "mts" | "cts" => ("lang/typescript", TYPESCRIPT),
                "jsx" | "tsx" => ("lang/react", REACT),
                "go" => ("lang/go", GO),
                "c" | "h" => ("lang/c", C),
                "cpp" | "cc" | "cxx" | "c++" | "hpp" | "hh" | "hxx" | "ino" => ("lang/cplusplus", CPP),
                "m" | "mm" => ("code-xml", BLUE),
                "cs" | "csx" | "fs" | "fsx" | "vb" | "csproj" | "sln" => ("lang/dotnet", DOTNET),
                "java" | "jar" | "class" => ("lang/openjdk", JAVA),
                "kt" | "kts" => ("lang/kotlin", KOTLIN),
                "rb" | "erb" | "gemspec" => ("lang/ruby", RUBY),
                "php" => ("lang/php", PHP),
                "html" | "htm" | "xhtml" => ("lang/html5", HTML),
                "xml" | "svg" | "xsl" | "plist" => ("code-xml", HTML),
                "css" => ("lang/css", CSS),
                "scss" | "sass" => ("lang/sass", SASS),
                "less" => ("lang/less", LESS),
                "md" | "markdown" | "mdx" => ("lang/markdown", MARKDOWN),
                "json" | "jsonc" | "json5" => ("lang/json", JSON),
                "sh" | "zsh" | "bash" | "fish" | "ksh" => ("lang/gnubash", SHELL),
                "ps1" | "bat" | "cmd" => ("terminal", BLUE),
                "lua" => ("lang/lua", LUA),
                "dart" => ("lang/dart", DART),
                "scala" | "sc" | "sbt" => ("lang/scala", SCALA),
                "hs" | "lhs" | "cabal" => ("lang/haskell", HASKELL),
                "ex" | "exs" | "heex" | "eex" => ("lang/elixir", ELIXIR),
                "zig" | "zon" => ("lang/zig", ZIG),
                "vue" => ("lang/vuedotjs", VUE),
                "svelte" => ("lang/svelte", SVELTE),
                "astro" => ("lang/astro", ASTRO),
                "yml" | "yaml" => ("lang/yaml", YAML),
                "toml" => ("lang/toml", TOML),
                "nix" => ("lang/nixos", NIX),
                "r" | "rmd" => ("lang/r", R),
                "jl" => ("lang/julia", JULIA),
                "pl" | "pm" => ("lang/perl", PERL),
                "ml" | "mli" => ("lang/ocaml", OCAML),
                "erl" | "hrl" => ("lang/erlang", ERLANG),
                "clj" | "cljs" | "cljc" | "edn" => ("lang/clojure", CLOJURE),
                "cmake" => ("lang/cmake", CMAKE),
                "graphql" | "gql" => ("lang/graphql", GRAPHQL),
                "sql" | "psql" => ("lang/postgresql", SQL),
                "db" | "sqlite" | "sqlite3" => ("lang/sqlite", SQL),
                "sol" => ("lang/solidity", SOLIDITY),
                "tf" | "tfvars" | "hcl" => ("lang/terraform", TERRAFORM),
                "tex" | "sty" | "cls" | "bib" => ("lang/latex", LATEX),
                "vim" => ("lang/vim", VIM),
                "ini" | "cfg" | "conf" | "xcconfig" | "env" | "properties" => ("settings", STEEL),
                "png" | "jpg" | "jpeg" | "gif" | "heic" | "webp" | "ico" | "icns" | "bmp" => ("image", MINT),
                "pdf" => ("file-text", RED),
                "mp4" | "mov" | "m4v" | "avi" | "mkv" | "webm" | "mpg" | "mpeg" | "3gp" => ("film", PURPLE),
                "zip" | "gz" | "tar" | "rar" | "7z" | "xz" | "bz2" | "zst" => ("archive", BROWN),
                "diff" | "patch" | "rej" => ("diff", GREEN),
                "csv" | "tsv" => ("file-text", GREEN),
                "lock" | "resolved" => ("lock", NEUTRAL),
                "txt" | "log" => ("file-text", NEUTRAL),
                _ => ("file", NEUTRAL),
            }
        }
    };
    (icon, pair.color(dark))
}
