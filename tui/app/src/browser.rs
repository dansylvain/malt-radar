//! Opening a fiche's URL in a browser from inside the TUI.
//!
//! This runs on a WSL2 workstation (per the project's dev environment),
//! so there's no single reliable opener: try the WSL-specific bridge
//! first, then the generic Linux one, then fall straight to Windows
//! Explorer, which also accepts a URL. No shell involved — each candidate
//! is spawned directly with the URL as its one argument, so there's no
//! injection surface even though the URL is server-provided (frontmatter
//! from `GET /fiches`, not something the user typed).

const OPENER_CANDIDATES: &[&str] = &["wslview", "xdg-open", "explorer.exe"];

/// Spawns the first available opener on `url`, detached (never awaited —
/// the TUI doesn't want to block on, or know about, the browser process).
/// Rejects anything that isn't `http(s)://` up front: this is the most
/// verbatim, clickable surface in the app, and a `file://` or bare path
/// slipping through would hand a local browser process a path traversal
/// for free.
pub fn open_url(url: &str) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!("URL invalide (doit commencer par http(s)://) : {url}"));
    }

    for opener in OPENER_CANDIDATES {
        if std::process::Command::new(opener).arg(url).spawn().is_ok() {
            return Ok(());
        }
    }

    Err(format!(
        "aucun ouvreur trouvé ({}) — copie l'URL manuellement",
        OPENER_CANDIDATES.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_http_schemes() {
        assert!(open_url("file:///etc/passwd").is_err());
        assert!(open_url("javascript:alert(1)").is_err());
        assert!(open_url("not a url").is_err());
    }
}
