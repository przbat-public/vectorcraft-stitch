//! VectorCraft Stitch's own settings folder (this fork's; its hook in `profile.rs` is listed in
//! FORK.md › Keeping up with upstream).
//!
//! The fork keeps its settings, `ui.json` and the folders beside it, in `VectorCraft Stitch`
//! (macOS, Windows) or `vectorcraft-stitch` (Linux and BSD), next to upstream VectorCraft's, so the
//! two apps never share `ui.json`: upstream resets a dock layout that holds a panel it doesn't know
//! (the Embroidery panel), and the fork's next start would then lose it too. The first time the
//! folder is wanted, VectorCraft's settings are copied into it (the project owner's choice), so
//! nothing a person set up is lost; from then on each app keeps its own. VectorCraft's folder is
//! never changed.
//!
//! Only the platform's folder moves: a folder given by `VECTORCRAFT_CONFIG_DIR`, a portable copy's
//! and the old DrawCraft one stay as upstream's rules pick them.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The fork's folder's name (macOS and Windows, then Linux and BSD), and upstream VectorCraft's,
/// beside which it lives.
pub const NAMES: (&str, &str) = ("VectorCraft Stitch", "vectorcraft-stitch");
pub const UPSTREAM_NAMES: (&str, &str) = ("VectorCraft", "vectorcraft");

/// What of VectorCraft's folder isn't copied: its logs (of its own runs), and its Data Recovery
/// copies, which VectorCraft recovers itself (the fork offering them too would bring one document
/// back twice).
pub const NOT_COPIED: [&str; 2] = ["logs", "Data Recovery"];

/// The settings file to use, given `chosen` (the one upstream's rules pick) and `upstream`
/// (VectorCraft's platform folder): in the fork's folder beside `upstream` when `chosen` is in it,
/// else `chosen` as it is. The first time in a process, the fork's folder is made from a copy of
/// VectorCraft's when it doesn't exist yet.
pub fn own(chosen: Option<PathBuf>, upstream: Option<PathBuf>) -> Option<PathBuf> {
    let chosen = chosen?;
    let Some((upstream, fork)) = upstream.and_then(|u| fork_beside(&u).map(|f| (u, f))) else { return Some(chosen) };
    if chosen.parent() != Some(upstream.as_path()) {
        return Some(chosen);
    }
    static FIRST: OnceLock<()> = OnceLock::new();
    FIRST.get_or_init(|| match adopt(&upstream, &fork) {
        Ok(true) => log::info!("copied VectorCraft's settings from {} to {}", upstream.display(), fork.display()),
        Ok(false) => {}
        // Only the log: the app starts with its defaults, and VectorCraft's folder is untouched.
        Err(e) => log::warn!("VectorCraft's settings in {} weren't copied to {}: {e}", upstream.display(), fork.display()),
    });
    Some(fork.join(chosen.file_name()?))
}

/// The fork's folder beside VectorCraft's folder `upstream`, `None` when `upstream` isn't one.
fn fork_beside(upstream: &Path) -> Option<PathBuf> {
    let name = upstream.file_name()?;
    let fork = if name == UPSTREAM_NAMES.0 {
        NAMES.0
    } else if name == UPSTREAM_NAMES.1 {
        NAMES.1
    } else {
        return None;
    };
    Some(upstream.with_file_name(fork))
}

/// Make the fork's folder `fork` from a copy of VectorCraft's `upstream` (all but [`NOT_COPIED`])
/// when `fork` doesn't exist and `upstream` does → whether it copied. The copy is made beside
/// `fork` and then renamed to it, so a copy cut short is never taken for the settings, and of two
/// processes starting at once only one's copy is kept.
fn adopt(upstream: &Path, fork: &Path) -> Result<bool, String> {
    if fork.exists() || !upstream.is_dir() {
        return Ok(false);
    }
    let name = fork.file_name().ok_or("no folder name")?.to_string_lossy().to_string();
    let copying = fork.with_file_name(format!("{name}.copying-{}", std::process::id()));
    std::fs::create_dir_all(&copying).map_err(|e| format!("{}: {e}", copying.display()))?;
    let copied = copy_into(upstream, &copying, true);
    let placed = copied.and_then(|()| std::fs::rename(&copying, fork).map_err(|e| format!("{}: {e}", fork.display())));
    if placed.is_err() {
        // Nothing half-copied stays behind; another process's copy may have won the rename.
        let _ = std::fs::remove_dir_all(&copying);
    }
    placed.map(|()| true)
}

/// Copy the contents of `from` into `to` (which exists), leaving out [`NOT_COPIED`] at the `top`.
/// A symbolic link is copied as what it points to.
fn copy_into(from: &Path, to: &Path, top: bool) -> Result<(), String> {
    for entry in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", from.display()))?;
        if top && NOT_COPIED.iter().any(|n| entry.file_name() == *n) {
            continue;
        }
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type().map_err(|e| format!("{}: {e}", src.display()))?;
        let done = if kind.is_dir() || (kind.is_symlink() && src.is_dir()) {
            std::fs::create_dir_all(&dst).map_err(|e| e.to_string()).and_then(|()| copy_into(&src, &dst, false))
        } else {
            std::fs::copy(&src, &dst).map(|_| ()).map_err(|e| e.to_string())
        };
        done.map_err(|e| format!("{}: {e}", src.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder to play in, removed by the test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vectorcraft-stitch-profile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The first time, VectorCraft's settings are copied into the fork's folder beside it (not its
    /// logs nor its Data Recovery copies) and VectorCraft's stay as they were; once the fork's
    /// folder exists, nothing is copied again.
    #[test]
    fn the_first_start_copies_vectorcrafts_settings() {
        let root = scratch("first");
        let upstream = root.join("VectorCraft");
        for (path, text) in
            [("ui.json", "{\"workspace\": \"Painting\"}"), ("Swatches/mine.ase", "swatches"), ("logs/old.log", "log"), ("Data Recovery/a.vc", "doc")]
        {
            let p = upstream.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        let fork = root.join("VectorCraft Stitch");
        assert_eq!(adopt(&upstream, &fork), Ok(true));
        assert_eq!(std::fs::read_to_string(fork.join("ui.json")).unwrap(), "{\"workspace\": \"Painting\"}");
        assert_eq!(std::fs::read_to_string(fork.join("Swatches/mine.ase")).unwrap(), "swatches");
        assert!(!fork.join("logs").exists() && !fork.join("Data Recovery").exists());
        assert!(upstream.join("logs/old.log").exists() && upstream.join("Data Recovery/a.vc").exists(), "VectorCraft's are untouched");
        // From then on each keeps its own.
        std::fs::write(fork.join("ui.json"), "{}").unwrap();
        assert_eq!(adopt(&upstream, &fork), Ok(false));
        assert_eq!(std::fs::read_to_string(fork.join("ui.json")).unwrap(), "{}");
        assert!(std::fs::read_dir(&root).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().contains(".copying-")), "no copy left over");
        // No VectorCraft folder: nothing to copy.
        let _ = std::fs::remove_dir_all(&fork);
        std::fs::remove_dir_all(&upstream).unwrap();
        assert_eq!(adopt(&upstream, &fork), Ok(false));
        assert!(!fork.exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Only a settings file in VectorCraft's platform folder moves into the fork's; an isolated or
    /// portable one, or DrawCraft's, stays where upstream's rules put it.
    #[test]
    fn only_the_platform_folder_moves() {
        let mac = PathBuf::from("/user/Library/Application Support/VectorCraft");
        assert_eq!(own(Some(mac.join("ui.json")), Some(mac.clone())), Some("/user/Library/Application Support/VectorCraft Stitch/ui.json".into()));
        let linux = PathBuf::from("/xdg-config/vectorcraft");
        assert_eq!(fork_beside(&linux), Some("/xdg-config/vectorcraft-stitch".into()));
        for chosen in ["/isolated/ui.json", "/usb/VectorCraftData/ui.json"] {
            assert_eq!(own(Some(chosen.into()), Some(mac.clone())), Some(chosen.into()), "{chosen}");
        }
        let drawcraft = PathBuf::from("/user/Library/Application Support/DrawCraft");
        assert_eq!(own(Some(drawcraft.join("ui.json")), Some(drawcraft.clone())), Some(drawcraft.join("ui.json")));
        assert_eq!(own(None, Some(mac)), None);
    }
}
