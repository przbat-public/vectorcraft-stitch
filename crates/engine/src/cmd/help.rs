//! Help → the project's GitHub repository. The commands return the URL; the frontend opens it.
//! This fork's build doesn't link to the ArtCraft community (its Discord, website and this app's
//! page there): `docs/brand/LICENSE-brand.txt` asks a modified version not to present itself as
//! theirs (FORK.md).

use serde_json::{Value, json};

use super::*;

/// The app's id on GitHub.
pub const APP_ID: &str = "vectorcraft";

/// This app's source repository.
pub fn github_url() -> String {
    format!("https://github.com/storytold/{APP_ID}")
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "help.github", "VectorCraft on GitHub", ["Help"], None, "{} → {url} source code, issues and releases", always, |_, _| url(github_url())),
        cmd!(query "help.links", "Links", [], None, "{} → {github}", always, |_, _| Ok(json!({ "github": github_url() }))),
    ]
}

fn url(u: String) -> Result<Value> {
    Ok(json!({ "url": u }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links() {
        let mut s = Session::new();
        assert_eq!(s.execute("help.github", &json!({})).unwrap()["url"], "https://github.com/storytold/vectorcraft");
        assert_eq!(s.execute("help.links", &json!({})).unwrap(), json!({"github": "https://github.com/storytold/vectorcraft"}));
        // No link to the ArtCraft community in this fork's build.
        for gone in ["help.discord", "help.website", "help.appPage"] {
            assert!(s.execute(gone, &json!({})).is_err(), "{gone}");
        }
    }
}
