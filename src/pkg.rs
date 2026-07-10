//! Mako package manager: resolve, lockfile, install, update, local publish.
//!
//! Layout:
//! - Manifest: `mako.toml` (`name`/`version` + `[dependencies]`)
//! - Lockfile: `mako.lock` (reproducible pins)
//! - Local registry: `$MAKO_REGISTRY` or `<project>/.mako/registry/<name>/<ver>/`
//! - Git cache: `<project>/.mako/deps/<name>/`

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::tooling::{
    git_dep_cache_abs, parse_manifest_deps, parse_semver, registry_resolve, registry_root,
    valid_dep_cache_name, version_satisfies, ManifestDep,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedPackage {
    pub name: String,
    pub version: String,
    pub source: String, // path | git | registry
    pub path: Option<String>,
    pub git: Option<String>,
    pub rev: Option<String>,
    pub tag: Option<String>,
    pub branch: Option<String>,
    pub content_hash: String,
}

#[derive(Debug, Clone, Default)]
pub struct Lockfile {
    pub version: u32,
    pub packages: Vec<LockedPackage>,
}

fn read_meta(text: &str) -> (Option<String>, Option<String>) {
    let mut name = None;
    let mut version = None;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('#') {
            continue;
        }
        if t.starts_with('[') {
            break;
        }
        if let Some(rest) = t.strip_prefix("name") {
            let rest = rest.trim().trim_start_matches('=').trim();
            let v = rest.trim_matches('"').trim_matches('\'').trim();
            if !v.is_empty() {
                name = Some(v.to_string());
            }
        } else if let Some(rest) = t.strip_prefix("version") {
            let rest = rest.trim().trim_start_matches('=').trim();
            let v = rest.trim_matches('"').trim_matches('\'').trim();
            if !v.is_empty() {
                version = Some(v.to_string());
            }
        }
    }
    (name, version)
}

fn simple_hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn hash_path_dep(full: &Path) -> String {
    if !full.exists() {
        return "missing".into();
    }
    if full.is_file() {
        return fs::read(full)
            .map(|b| simple_hash(&b))
            .unwrap_or_else(|_| "missing".into());
    }
    let mut buf = Vec::new();
    let manifest = full.join("mako.toml");
    if let Ok(b) = fs::read(&manifest) {
        buf.extend_from_slice(&b);
    }
    let mut mko: Vec<_> = fs::read_dir(full)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("mko"))
        .collect();
    mko.sort();
    for p in mko {
        if let Ok(b) = fs::read(&p) {
            buf.extend_from_slice(p.to_string_lossy().as_bytes());
            buf.extend_from_slice(&b);
        }
    }
    if buf.is_empty() {
        "empty".into()
    } else {
        simple_hash(&buf)
    }
}

fn path_dep_version(full: &Path) -> Option<String> {
    let manifest = if full.is_file() {
        full.parent()?.join("mako.toml")
    } else {
        full.join("mako.toml")
    };
    let text = fs::read_to_string(manifest).ok()?;
    read_meta(&text).1
}

/// Effective registry root: `MAKO_REGISTRY` or `<project>/.mako/registry`.
pub fn effective_registry(project: &Path) -> PathBuf {
    if let Ok(p) = std::env::var("MAKO_REGISTRY") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    registry_root(project)
}

fn registry_resolve_in(reg: &Path, name: &str, req: &str) -> Result<(PathBuf, String), String> {
    let root = reg.join(name);
    if !root.is_dir() {
        return Err(format!(
            "no local registry entry for `{name}` under {}",
            reg.display()
        ));
    }
    let mut best: Option<(u64, u64, u64, PathBuf, String)> = None;
    let rd = fs::read_dir(&root).map_err(|e| format!("read registry: {e}"))?;
    for ent in rd.flatten() {
        let ver_name = ent.file_name().to_string_lossy().to_string();
        if !version_satisfies(&ver_name, req) {
            continue;
        }
        let Some(sv) = parse_semver(&ver_name) else {
            continue;
        };
        let path = ent.path();
        if !path.join("mako.toml").exists() {
            continue;
        }
        match &best {
            None => best = Some((sv.0, sv.1, sv.2, path, ver_name)),
            Some((a, b, c, _, _)) if (sv.0, sv.1, sv.2) > (*a, *b, *c) => {
                best = Some((sv.0, sv.1, sv.2, path, ver_name));
            }
            _ => {}
        }
    }
    best.map(|(_, _, _, p, v)| (p, v))
        .ok_or_else(|| format!("no version of `{name}` satisfies `{req}` in local registry"))
}

fn fetch_git(project: &Path, dep: &ManifestDep, offline: bool) -> Result<PathBuf, String> {
    if !valid_dep_cache_name(&dep.name) {
        return Err(format!(
            "git dep `{}` has an invalid cache name (allowed: letters, digits, `_`, `-`, `.`)",
            dep.name
        ));
    }
    let url = dep
        .git
        .as_deref()
        .ok_or_else(|| format!("dep `{}` has no git URL", dep.name))?;
    let dest = git_dep_cache_abs(project, &dep.name);
    if dest.exists() {
        return Ok(dest);
    }
    if offline {
        return Err(format!(
            "offline mode: git dep `{}` is not cached at {}",
            dep.name,
            dest.display()
        ));
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    let mut cmd = Command::new("git");
    cmd.arg("-c")
        .arg("core.hooksPath=/dev/null")
        .arg("clone")
        .arg("--depth")
        .arg("1");
    if let Some(b) = &dep.branch {
        cmd.arg("--branch").arg(b);
    } else if let Some(t) = &dep.tag {
        cmd.arg("--branch").arg(t);
    }
    cmd.arg(url).arg(&dest);
    let status = cmd.status().map_err(|e| format!("git clone: {e}"))?;
    if !status.success() {
        let _ = fs::remove_dir_all(&dest);
        return Err(format!("git clone failed for `{url}`"));
    }
    if let Some(r) = &dep.rev {
        let st = Command::new("git")
            .args(["-C"])
            .arg(&dest)
            .args(["checkout", "--force", r])
            .status()
            .map_err(|e| format!("git checkout: {e}"))?;
        if !st.success() {
            return Err(format!("git checkout {r} failed"));
        }
    }
    Ok(dest)
}

/// Resolve one direct dep to an on-disk root + locked metadata.
fn resolve_one(
    project: &Path,
    dep: &ManifestDep,
    prefer_highest: bool,
    offline: bool,
) -> Result<LockedPackage, String> {
    let _ = prefer_highest;
    let reg = effective_registry(project);
    if let Some(p) = &dep.path {
        let full = project.join(p);
        if !full.exists() {
            return Err(format!(
                "path dep `{}` MISSING: {}",
                dep.name,
                full.display()
            ));
        }
        let ver = path_dep_version(&full)
            .or_else(|| dep.version.clone())
            .unwrap_or_else(|| "0.0.0".into());
        if let Some(req) = &dep.version {
            if !version_satisfies(&ver, req) {
                return Err(format!(
                    "path dep `{}` version {ver} does not satisfy `{req}`",
                    dep.name
                ));
            }
        }
        return Ok(LockedPackage {
            name: dep.name.clone(),
            version: ver,
            source: "path".into(),
            path: Some(p.clone()),
            git: None,
            rev: None,
            tag: None,
            branch: None,
            content_hash: hash_path_dep(&full),
        });
    }
    if dep.git.is_some() {
        let dest = fetch_git(project, dep, offline)?;
        let ver = path_dep_version(&dest)
            .or_else(|| dep.version.clone())
            .unwrap_or_else(|| "0.0.0".into());
        return Ok(LockedPackage {
            name: dep.name.clone(),
            version: ver,
            source: "git".into(),
            path: Some(
                dest.strip_prefix(project)
                    .unwrap_or(&dest)
                    .to_string_lossy()
                    .to_string(),
            ),
            git: dep.git.clone(),
            rev: dep.rev.clone(),
            tag: dep.tag.clone(),
            branch: dep.branch.clone(),
            content_hash: hash_path_dep(&dest),
        });
    }
    if let Some(req) = &dep.version {
        // Prefer project registry, then MAKO_REGISTRY / effective.
        let (dir, ver) = if let Ok(r) = registry_resolve(project, &dep.name, req) {
            let v = path_dep_version(&r)
                .unwrap_or_else(|| req.trim_start_matches(['^', '~']).to_string());
            (r, v)
        } else {
            registry_resolve_in(&reg, &dep.name, req)?
        };
        // Copy into project cache for reproducible builds if outside project.
        let cache = project.join(".mako").join("deps").join(&dep.name);
        if !cache.exists() {
            if let Some(parent) = cache.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
            }
            copy_dir_recursive(&dir, &cache)?;
        }
        let use_path = if cache.exists() { &cache } else { &dir };
        let ver = path_dep_version(use_path).unwrap_or(ver);
        return Ok(LockedPackage {
            name: dep.name.clone(),
            version: ver,
            source: "registry".into(),
            path: Some(
                use_path
                    .strip_prefix(project)
                    .unwrap_or(use_path)
                    .to_string_lossy()
                    .to_string(),
            ),
            git: None,
            rev: None,
            tag: None,
            branch: None,
            content_hash: hash_path_dep(use_path),
        });
    }
    Err(format!(
        "dependency `{}` needs path, git, or version (registry)",
        dep.name
    ))
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("mkdir {}: {e}", dst.display()))?;
    for ent in fs::read_dir(src).map_err(|e| format!("read {}: {e}", src.display()))? {
        let ent = ent.map_err(|e| format!("readdir: {e}"))?;
        let from = ent.path();
        let to = dst.join(ent.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to).map_err(|e| format!("copy {}: {e}", from.display()))?;
        }
    }
    Ok(())
}

/// Transitive resolve with SemVer conflict detection (Cargo-like: one version per name).
pub fn resolve_graph_with_options(
    project: &Path,
    update: bool,
    offline: bool,
) -> Result<Lockfile, String> {
    let manifest = project.join("mako.toml");
    if !manifest.exists() {
        return Err("no mako.toml — run `mako pkg init` first".into());
    }
    let text = fs::read_to_string(&manifest).map_err(|e| format!("read mako.toml: {e}"))?;
    let (root_name, root_ver) = read_meta(&text);
    let root_name = root_name.unwrap_or_else(|| "root".into());
    let root_ver = root_ver.unwrap_or_else(|| "0.1.0".into());

    let existing = if !update {
        read_lockfile(&project.join("mako.lock")).ok()
    } else {
        None
    };

    let mut resolved: BTreeMap<String, LockedPackage> = BTreeMap::new();
    let mut reqs: HashMap<String, String> = HashMap::new(); // name → tightest req seen
    let mut queue: Vec<(PathBuf, ManifestDep)> = Vec::new();

    for d in parse_manifest_deps(&text) {
        queue.push((project.to_path_buf(), d));
    }

    let mut visiting = HashSet::new();
    while let Some((from, dep)) = queue.pop() {
        let key = dep.name.clone();
        if let Some(req) = &dep.version {
            if let Some(prev) = reqs.get(&key) {
                // Conflict if neither satisfies the other as a concrete version pick.
                // Keep both constraints: resolved version must satisfy all.
                if prev != req {
                    reqs.insert(key.clone(), format!("{prev}&&{req}"));
                }
            } else {
                reqs.insert(key.clone(), req.clone());
            }
        }

        if resolved.contains_key(&key) && !update {
            // Already locked — verify constraints.
            if let Some(lp) = resolved.get(&key) {
                if let Some(req) = &dep.version {
                    for part in req.split("&&") {
                        if !version_satisfies(&lp.version, part.trim()) {
                            return Err(format!(
                                "version conflict for `{}`: locked {} does not satisfy `{part}` (from {})",
                                key,
                                lp.version,
                                from.display()
                            ));
                        }
                    }
                }
            }
            continue;
        }

        // Prefer existing lock entry when not updating.
        if !update {
            if let Some(lock) = &existing {
                if let Some(lp) = lock.packages.iter().find(|p| p.name == key) {
                    if let Some(req) = &dep.version {
                        for part in req.split("&&") {
                            if !version_satisfies(&lp.version, part.trim()) {
                                return Err(format!(
                                    "lockfile `{}` @ {} does not satisfy `{part}` — run `mako pkg update`",
                                    key, lp.version
                                ));
                            }
                        }
                    }
                    // Ensure on disk
                    if let Some(p) = &lp.path {
                        let full = project.join(p);
                        if !full.exists() && lp.source == "git" {
                            if offline {
                                return Err(format!(
                                    "offline mode: locked git dep `{}` is missing at {}",
                                    key,
                                    full.display()
                                ));
                            }
                            let _ = fetch_git(project, &dep, false);
                        }
                        if !full.exists() && lp.source == "registry" {
                            let _ = resolve_one(project, &dep, true, offline)?;
                        }
                    }
                    resolved.insert(key.clone(), lp.clone());
                    // Transitive from locked path
                    if let Some(p) = &lp.path {
                        let dir = project.join(p);
                        let next = if dir.is_file() {
                            dir.parent().map(|x| x.to_path_buf())
                        } else {
                            Some(dir)
                        };
                        if let Some(nd) = next {
                            if let Ok(t) = fs::read_to_string(nd.join("mako.toml")) {
                                for td in parse_manifest_deps(&t) {
                                    if visiting.insert(format!("{}->{}", key, td.name)) {
                                        queue.push((nd.clone(), td));
                                    }
                                }
                            }
                        }
                    }
                    continue;
                }
            }
        }

        let locked = resolve_one(project, &dep, true, offline)?;
        if let Some(combo) = reqs.get(&key) {
            for part in combo.split("&&") {
                let part = part.trim();
                if !part.is_empty() && !version_satisfies(&locked.version, part) {
                    return Err(format!(
                        "version conflict for `{}`: resolved {} does not satisfy `{part}`",
                        key, locked.version
                    ));
                }
            }
        }
        if let Some(prev) = resolved.get(&key) {
            if prev.version != locked.version {
                return Err(format!(
                    "version conflict for `{}`: need both {} and {} — pin one version in mako.toml",
                    key, prev.version, locked.version
                ));
            }
        }
        resolved.insert(key.clone(), locked.clone());

        if let Some(p) = &locked.path {
            let dir = project.join(p);
            let next = if dir.is_file() {
                dir.parent().map(|x| x.to_path_buf())
            } else {
                Some(dir)
            };
            if let Some(nd) = next {
                if let Ok(t) = fs::read_to_string(nd.join("mako.toml")) {
                    for td in parse_manifest_deps(&t) {
                        if visiting.insert(format!("{}->{}", key, td.name)) {
                            queue.push((nd.clone(), td));
                        }
                    }
                }
            }
        }
    }

    let mut packages: Vec<LockedPackage> = resolved.into_values().collect();
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    packages.insert(
        0,
        LockedPackage {
            name: root_name,
            version: root_ver,
            source: "path".into(),
            path: Some(".".into()),
            git: None,
            rev: None,
            tag: None,
            branch: None,
            content_hash: simple_hash(&fs::read(&manifest).unwrap_or_default()),
        },
    );

    Ok(Lockfile {
        version: 1,
        packages,
    })
}

#[allow(dead_code)]
pub fn resolve_graph(project: &Path, update: bool) -> Result<Lockfile, String> {
    resolve_graph_with_options(project, update, false)
}

pub fn write_lockfile(project: &Path, lock: &Lockfile) -> Result<PathBuf, String> {
    let mut out = String::from(
        "# mako.lock — reproducible dependency pin\n# Generated by `mako pkg install` / `mako pkg lock`\n",
    );
    out.push_str(&format!("version = {}\n", lock.version));
    for p in &lock.packages {
        out.push_str("\n[[package]]\n");
        out.push_str(&format!("name = \"{}\"\n", p.name));
        out.push_str(&format!("version = \"{}\"\n", p.version));
        out.push_str(&format!("source = \"{}\"\n", p.source));
        if let Some(path) = &p.path {
            out.push_str(&format!("path = \"{path}\"\n"));
        }
        if let Some(g) = &p.git {
            out.push_str(&format!("git = \"{g}\"\n"));
        }
        if let Some(r) = &p.rev {
            out.push_str(&format!("rev = \"{r}\"\n"));
        }
        if let Some(t) = &p.tag {
            out.push_str(&format!("tag = \"{t}\"\n"));
        }
        if let Some(b) = &p.branch {
            out.push_str(&format!("branch = \"{b}\"\n"));
        }
        out.push_str(&format!("content_hash = \"{}\"\n", p.content_hash));
    }
    let path = project.join("mako.lock");
    fs::write(&path, out).map_err(|e| format!("write mako.lock: {e}"))?;
    Ok(path)
}

pub fn read_lockfile(path: &Path) -> Result<Lockfile, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut version = 1u32;
    let mut packages = Vec::new();
    let mut cur: Option<LockedPackage> = None;
    let flush = |cur: &mut Option<LockedPackage>, packages: &mut Vec<LockedPackage>| {
        if let Some(p) = cur.take() {
            if !p.name.is_empty() {
                packages.push(p);
            }
        }
    };
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if t.starts_with("version") && !t.contains('"') {
            if let Some(n) = t.split('=').nth(1) {
                version = n.trim().parse().unwrap_or(1);
            }
            continue;
        }
        if t == "[[package]]" {
            flush(&mut cur, &mut packages);
            cur = Some(LockedPackage {
                name: String::new(),
                version: "0.0.0".into(),
                source: "path".into(),
                path: None,
                git: None,
                rev: None,
                tag: None,
                branch: None,
                content_hash: String::new(),
            });
            continue;
        }
        let Some(pkg) = cur.as_mut() else {
            continue;
        };
        let Some((k, v)) = t.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim().trim_matches('"').to_string();
        match k {
            "name" => pkg.name = v,
            "version" => pkg.version = v,
            "source" => pkg.source = v,
            "path" => pkg.path = Some(v),
            "git" => pkg.git = Some(v),
            "rev" => pkg.rev = Some(v),
            "tag" => pkg.tag = Some(v),
            "branch" => pkg.branch = Some(v),
            "content_hash" => pkg.content_hash = v,
            _ => {}
        }
    }
    flush(&mut cur, &mut packages);
    Ok(Lockfile { version, packages })
}

pub fn pkg_install(project: &Path, offline: bool) -> Result<(), String> {
    let lock = resolve_graph_with_options(project, false, offline)?;
    let path = write_lockfile(project, &lock)?;
    println!(
        "mako pkg install{}: {} packages → {}",
        if offline { " --offline" } else { "" },
        lock.packages.len(),
        path.display()
    );
    for p in &lock.packages {
        if p.path.as_deref() == Some(".") {
            continue;
        }
        println!("  {} {} ({})", p.name, p.version, p.source);
    }
    Ok(())
}

pub fn pkg_update(project: &Path, offline: bool) -> Result<(), String> {
    let lock = resolve_graph_with_options(project, true, offline)?;
    let path = write_lockfile(project, &lock)?;
    println!(
        "mako pkg update{}: refreshed {} packages → {}",
        if offline { " --offline" } else { "" },
        lock.packages.len(),
        path.display()
    );
    for p in &lock.packages {
        if p.path.as_deref() == Some(".") {
            continue;
        }
        println!("  {} {} ({})", p.name, p.version, p.source);
    }
    Ok(())
}

#[allow(dead_code)]
pub fn pkg_lock(project: &Path, offline: bool) -> Result<(), String> {
    pkg_install(project, offline)
}

/// Publish current package into local registry (`MAKO_REGISTRY` or `.mako/registry`).
pub fn pkg_publish(project: &Path) -> Result<(), String> {
    let manifest = project.join("mako.toml");
    if !manifest.exists() {
        return Err("no mako.toml — run `mako pkg init` first".into());
    }
    let text = fs::read_to_string(&manifest).map_err(|e| format!("read: {e}"))?;
    let (name, version) = read_meta(&text);
    let name = name.ok_or_else(|| "mako.toml missing `name`".to_string())?;
    let version = version.ok_or_else(|| "mako.toml missing `version`".to_string())?;
    let reg = effective_registry(project);
    let dest = reg.join(&name).join(&version);
    if dest.exists() {
        fs::remove_dir_all(&dest).map_err(|e| format!("clear old publish: {e}"))?;
    }
    fs::create_dir_all(&dest).map_err(|e| format!("mkdir: {e}"))?;
    fs::copy(&manifest, dest.join("mako.toml")).map_err(|e| format!("copy toml: {e}"))?;
    // Copy .mko sources (non-recursive top-level + common lib/)
    for ent in fs::read_dir(project).map_err(|e| format!("readdir: {e}"))? {
        let ent = ent.map_err(|e| e.to_string())?;
        let p = ent.path();
        let name_os = ent.file_name();
        let n = name_os.to_string_lossy();
        if n == ".mako" || n == "mako.lock" || n.starts_with('.') {
            continue;
        }
        if p.is_file() && p.extension().and_then(|x| x.to_str()) == Some("mko") {
            fs::copy(&p, dest.join(&*n)).map_err(|e| format!("copy: {e}"))?;
        } else if p.is_dir() && (n == "src" || n == "lib") {
            copy_dir_recursive(&p, &dest.join(&*n))?;
        }
    }
    println!("published {name}@{version} → {}", dest.display());
    println!(
        "hint: depend with `\"{name}\" = {{ version = \"^{version}\" }}` then `mako pkg install`"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn lockfile_roundtrip() {
        let dir = env::temp_dir().join(format!("mako_pkg_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("mako.toml"),
            "name = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\n",
        )
        .unwrap();
        let lock = resolve_graph(&dir, true).unwrap();
        let path = write_lockfile(&dir, &lock).unwrap();
        let back = read_lockfile(&path).unwrap();
        assert_eq!(back.packages.len(), 1);
        assert_eq!(back.packages[0].name, "app");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn registry_publish_and_resolve() {
        let dir = env::temp_dir().join(format!("mako_pkg_reg_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let lib = dir.join("lib");
        let app = dir.join("app");
        fs::create_dir_all(&lib).unwrap();
        fs::create_dir_all(&app).unwrap();
        fs::write(
            lib.join("mako.toml"),
            "name = \"util\"\nversion = \"1.2.0\"\n",
        )
        .unwrap();
        fs::write(
            lib.join("lib.mko"),
            "fn add(a: int, b: int) -> int { a + b }\n",
        )
        .unwrap();
        pkg_publish(&lib).unwrap();
        // Point app registry at lib's published tree via MAKO_REGISTRY
        let reg = effective_registry(&lib);
        fs::write(
            app.join("mako.toml"),
            format!(
                "name = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\n\"util\" = {{ version = \"^1.0.0\" }}\n"
            ),
        )
        .unwrap();
        // Copy registry into app or set env — copy for isolation
        let app_reg = app.join(".mako").join("registry");
        copy_dir_recursive(&reg, &app_reg).unwrap();
        let lock = resolve_graph(&app, true).unwrap();
        assert!(lock
            .packages
            .iter()
            .any(|p| p.name == "util" && p.version.starts_with("1.")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn offline_git_requires_cached_dep() {
        let dir = env::temp_dir().join(format!("mako_pkg_offline_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("mako.toml"),
            r#"name = "app"
version = "0.1.0"

[dependencies]
"util" = { git = "https://example.invalid/util.git", version = "1.2.0" }
"#,
        )
        .unwrap();

        let err = resolve_graph_with_options(&dir, true, true).unwrap_err();
        assert!(err.contains("offline mode"), "unexpected: {err}");

        let cached = dir.join(".mako").join("deps").join("util");
        fs::create_dir_all(&cached).unwrap();
        fs::write(
            cached.join("mako.toml"),
            "name = \"util\"\nversion = \"1.2.0\"\n",
        )
        .unwrap();
        fs::write(cached.join("lib.mko"), "fn ok() -> int { 1 }\n").unwrap();

        let lock = resolve_graph_with_options(&dir, true, true).unwrap();
        assert!(lock
            .packages
            .iter()
            .any(|p| p.name == "util" && p.source == "git" && p.version == "1.2.0"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn semver_conflict_detected() {
        let dir = env::temp_dir().join(format!("mako_pkg_conf_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let a = dir.join("a");
        let b = dir.join("b");
        let app = dir.join("app");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::create_dir_all(&app).unwrap();
        fs::write(
            a.join("mako.toml"),
            "name = \"leaf\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        fs::write(
            b.join("mako.toml"),
            "name = \"leaf\"\nversion = \"2.0.0\"\n",
        )
        .unwrap();
        fs::write(
            app.join("mako.toml"),
            r#"name = "app"
version = "0.1.0"

[dependencies]
"x" = { path = "../a", version = "1.0.0" }
"y" = { path = "../b", version = "2.0.0" }
"#,
        )
        .unwrap();
        // Different package names x/y — no conflict. Conflict needs same name.
        fs::write(
            app.join("mako.toml"),
            r#"name = "app"
version = "0.1.0"

[dependencies]
"leaf" = { path = "../a", version = "^1.0.0" }
"#,
        )
        .unwrap();
        // Add a second manifest that also wants leaf@2 via a mid package
        let mid = dir.join("mid");
        fs::create_dir_all(&mid).unwrap();
        fs::write(
            mid.join("mako.toml"),
            r#"name = "mid"
version = "0.1.0"

[dependencies]
"leaf" = { path = "../b", version = "^2.0.0" }
"#,
        )
        .unwrap();
        fs::write(
            app.join("mako.toml"),
            r#"name = "app"
version = "0.1.0"

[dependencies]
"leaf" = { path = "../a", version = "^1.0.0" }
"mid" = { path = "../mid", version = "0.1.0" }
"#,
        )
        .unwrap();
        let err = resolve_graph(&app, true).unwrap_err();
        assert!(
            err.contains("conflict") || err.contains("does not satisfy"),
            "unexpected: {err}"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
