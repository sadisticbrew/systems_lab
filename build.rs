use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

const UNISTD: &str = "/usr/include/asm/unistd_64.h";
const ERRNO_HEADERS: [&str; 2] = ["/usr/include/asm-generic/errno-base.h", "/usr/include/asm-generic/errno.h"];

fn main() {
    syscall_table();
    errno_table();
    println!("cargo:rerun-if-changed=experiments");
    viewer_assets();
}

/// `$OUT_DIR/syscalls.rs`: `SYSCALL_NAMES`, sorted by number.
fn syscall_table() {
    println!("cargo:rerun-if-changed={UNISTD}");
    let mut table: Vec<(u64, String)> = match fs::read_to_string(UNISTD) {
        Ok(text) => text
            .lines()
            .filter_map(|line| {
                let rest = line.strip_prefix("#define __NR_")?;
                let mut parts = rest.split_whitespace();
                let name = parts.next()?;
                let nr = parts.next()?.parse().ok()?;
                Some((nr, name.to_string()))
            })
            .collect(),
        Err(err) => {
            println!("cargo:warning=cannot read {UNISTD} ({err}); syscall names will be numeric");
            Vec::new()
        }
    };
    table.sort();
    table.dedup_by_key(|(nr, _)| *nr);
    let mut out = String::from("pub static SYSCALL_NAMES: &[(u64, &str)] = &[\n");
    for (nr, name) in &table {
        out.push_str(&format!("    ({nr}, {name:?}),\n"));
    }
    out.push_str("];\n");
    let dest = PathBuf::from(env::var("OUT_DIR").unwrap()).join("syscalls.rs");
    fs::write(dest, out).unwrap();
}

/// `$OUT_DIR/errno.rs`: `ERRNO_NAMES` (number, name, text), sorted by number, from the
/// `#define NAME NUMBER /* TEXT */` lines of the asm-generic errno headers.
fn errno_table() {
    let mut table: Vec<(i64, String, String)> = Vec::new();
    for header in ERRNO_HEADERS {
        println!("cargo:rerun-if-changed={header}");
        match fs::read_to_string(header) {
            Ok(text) => table.extend(text.lines().filter_map(|line| {
                let rest = line.strip_prefix("#define")?;
                let mut parts = rest.split_whitespace();
                let name = parts.next()?;
                let nr = parts.next()?.parse().ok()?;
                let text = rest.split_once("/*")?.1.split_once("*/")?.0.trim();
                Some((nr, name.to_string(), text.to_string()))
            })),
            Err(err) => println!("cargo:warning=cannot read {header} ({err}); errno names will be numeric"),
        }
    }
    table.sort_by_key(|(nr, ..)| *nr);
    table.dedup_by_key(|(nr, ..)| *nr);
    let mut out = String::from("pub static ERRNO_NAMES: &[(i64, &str, &str)] = &[\n");
    for (nr, name, text) in &table {
        out.push_str(&format!("    ({nr}, {name:?}, {text:?}),\n"));
    }
    out.push_str("];\n");
    let dest = PathBuf::from(env::var("OUT_DIR").unwrap()).join("errno.rs");
    fs::write(dest, out).unwrap();
}

/// Builds `web/dist` with bun, or leaves a placeholder so rust-embed compiles.
fn viewer_assets() {
    for p in [
        "web/src",
        "web/index.html",
        "web/package.json",
        "web/vite.config.ts",
        "web/bun.lock",
        "web/svelte.config.js",
        "web/tsconfig.json",
    ] {
        println!("cargo:rerun-if-changed={p}");
    }
    println!("cargo:rerun-if-env-changed=SYSTEMSLAB_SKIP_WEB");
    let web = Path::new("web");
    let skip = env::var("SYSTEMSLAB_SKIP_WEB").is_ok_and(|v| v == "1");
    if !skip && web.join("package.json").exists() {
        let bun = find_bun().expect("bun is required to build the SystemsLab viewer");
        run(&bun, &["install", "--frozen-lockfile"], web);
        run(&bun, &["run", "build"], web);
        return;
    }
    let dist = web.join("dist");
    if !dist.exists() {
        fs::create_dir_all(&dist).unwrap();
        fs::write(
            dist.join("index.html"),
            "<!doctype html><title>SystemsLab</title><p>viewer not built</p>\n",
        )
        .unwrap();
    }
}

fn find_bun() -> Option<PathBuf> {
    let on_path = env::var_os("PATH")
        .into_iter()
        .flat_map(|p| env::split_paths(&p).collect::<Vec<_>>())
        .map(|dir| dir.join("bun"));
    let home = env::var_os("HOME").map(|h| PathBuf::from(h).join(".bun/bin/bun"));
    on_path.chain(home).find(|p| p.is_file())
}

fn run(bun: &Path, args: &[&str], dir: &Path) {
    let status = Command::new(bun)
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap_or_else(|e| panic!("running bun {}: {e}", args.join(" ")));
    assert!(status.success(), "bun {} failed in web/ ({status})", args.join(" "));
}
