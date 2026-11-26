use std::{
    env, fs,
    path::{Path, PathBuf},
};

// Detected by Start Menu shortcut, known folders, or Store package. "=" means exact name.
struct Known {
    id: &'static str,
    shortcuts: &'static [&'static str],
    paths: &'static [&'static str],
    packages: &'static [&'static str],
}

const KNOWN: &[Known] = &[
    Known {
        id: "discord",
        shortcuts: &["=discord"],
        paths: &["%LOCALAPPDATA%\\Discord"],
        packages: &[],
    },
    Known {
        id: "slack",
        shortcuts: &["=slack"],
        paths: &["%LOCALAPPDATA%\\slack"],
        packages: &[],
    },
    Known {
        id: "teams",
        shortcuts: &["microsoft teams"],
        paths: &["%LOCALAPPDATA%\\Microsoft\\Teams"],
        packages: &["msteams_"],
    },
    Known {
        id: "whatsapp",
        shortcuts: &["=whatsapp"],
        paths: &[],
        packages: &["5319275a.whatsappdesktop_"],
    },
    Known {
        id: "outlook",
        shortcuts: &["outlook"],
        paths: &["%ProgramFiles%\\Microsoft Office\\root\\Office16\\OUTLOOK.EXE"],
        packages: &["microsoft.outlookforwindows_"],
    },
    Known {
        id: "word",
        shortcuts: &["=word", "microsoft word"],
        paths: &["%ProgramFiles%\\Microsoft Office\\root\\Office16\\WINWORD.EXE"],
        packages: &[],
    },
    Known {
        id: "notion",
        shortcuts: &["=notion"],
        paths: &["%LOCALAPPDATA%\\Programs\\Notion"],
        packages: &[],
    },
    Known {
        id: "vscode",
        shortcuts: &["visual studio code"],
        paths: &[
            "%LOCALAPPDATA%\\Programs\\Microsoft VS Code",
            "%ProgramFiles%\\Microsoft VS Code",
        ],
        packages: &[],
    },
    Known {
        id: "cursor",
        shortcuts: &["=cursor"],
        paths: &["%LOCALAPPDATA%\\Programs\\cursor"],
        packages: &[],
    },
    Known {
        id: "claude",
        shortcuts: &["=claude"],
        paths: &["%LOCALAPPDATA%\\AnthropicClaude"],
        packages: &["claude_"],
    },
    Known {
        id: "chrome",
        shortcuts: &["google chrome"],
        paths: &[
            "%ProgramFiles%\\Google\\Chrome\\Application\\chrome.exe",
            "%ProgramFiles(x86)%\\Google\\Chrome\\Application\\chrome.exe",
            "%LOCALAPPDATA%\\Google\\Chrome\\Application\\chrome.exe",
        ],
        packages: &[],
    },
    Known {
        id: "figma",
        shortcuts: &["=figma"],
        paths: &["%LOCALAPPDATA%\\Figma"],
        packages: &[],
    },
    Known {
        id: "github",
        shortcuts: &["github desktop"],
        paths: &["%LOCALAPPDATA%\\GitHubDesktop"],
        packages: &[],
    },
    Known {
        id: "jetbrains",
        shortcuts: &[
            "intellij", "pycharm", "webstorm", "rider", "clion", "goland", "phpstorm", "rubymine",
            "datagrip",
        ],
        paths: &[],
        packages: &[],
    },
    Known {
        id: "visualstudio",
        shortcuts: &["visual studio 2"],
        paths: &[],
        packages: &[],
    },
    Known {
        id: "notepad",
        shortcuts: &[],
        paths: &["%WINDIR%\\notepad.exe"],
        packages: &[],
    },
];

fn expand(path: &str) -> Option<PathBuf> {
    let mut out = String::new();
    let mut rest = path;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let end = after.find('%')?;
        out.push_str(&env::var(&after[..end]).ok()?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(PathBuf::from(out))
}

fn shortcut_names(dir: &Path, depth: u32, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth > 0 {
                shortcut_names(&path, depth - 1, out);
            }
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
        {
            if let Some(stem) = path.file_stem() {
                out.push(stem.to_string_lossy().to_lowercase());
            }
        }
    }
}

fn start_menu() -> Vec<String> {
    let mut names = Vec::new();
    for var in ["ProgramData", "APPDATA"] {
        if let Ok(base) = env::var(var) {
            shortcut_names(
                &Path::new(&base).join("Microsoft\\Windows\\Start Menu\\Programs"),
                3,
                &mut names,
            );
        }
    }
    names
}

fn store_packages() -> Vec<String> {
    let Some(dir) = expand("%LOCALAPPDATA%\\Packages") else {
        return Vec::new();
    };
    fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().to_lowercase())
                .collect()
        })
        .unwrap_or_default()
}

fn shortcut_matches(pattern: &str, name: &str) -> bool {
    match pattern.strip_prefix('=') {
        Some(exact) => name == exact,
        None => name.contains(pattern),
    }
}

fn is_installed(app: &Known, shortcuts: &[String], packages: &[String]) -> bool {
    app.shortcuts
        .iter()
        .any(|p| shortcuts.iter().any(|name| shortcut_matches(p, name)))
        || app
            .paths
            .iter()
            .any(|p| expand(p).is_some_and(|path| path.exists()))
        || app
            .packages
            .iter()
            .any(|p| packages.iter().any(|name| name.starts_with(p)))
}

pub fn installed() -> Vec<&'static str> {
    let (shortcuts, packages) = (start_menu(), store_packages());
    KNOWN
        .iter()
        .filter(|app| is_installed(app, &shortcuts, &packages))
        .map(|app| app.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(id: &str) -> &'static Known {
        KNOWN.iter().find(|k| k.id == id).unwrap()
    }

    #[test]
    fn a_shortcut_name_can_be_exact_or_partial() {
        assert!(shortcut_matches("=word", "word"));
        assert!(!shortcut_matches("=word", "wordpad"));
        assert!(shortcut_matches(
            "visual studio code",
            "visual studio code - insiders"
        ));
        assert!(!shortcut_matches("visual studio 2", "visual studio code"));
    }

    #[test]
    fn apps_are_found_by_shortcut_or_store_package_and_not_otherwise() {
        let shortcuts = vec![
            "visual studio 2022".to_string(),
            "pycharm community".to_string(),
        ];
        let packages = vec!["5319275a.whatsappdesktop_cv1g1gvanyjgm".to_string()];
        assert!(is_installed(find("visualstudio"), &shortcuts, &packages));
        assert!(is_installed(find("jetbrains"), &shortcuts, &packages));
        assert!(is_installed(find("whatsapp"), &shortcuts, &packages));
        assert!(!is_installed(find("figma"), &shortcuts, &packages));
        assert!(!is_installed(find("whatsapp"), &[], &[]));
    }

    #[test]
    fn variables_in_paths_are_filled_in_and_a_missing_one_gives_nothing() {
        assert!(expand("%WINDIR%\\notepad.exe").is_some());
        assert!(expand("%NO_SUCH_VARIABLE_X%\\a").is_none());
        assert_eq!(expand("plain\\path"), Some(PathBuf::from("plain\\path")));
    }

    #[test]
    fn notepad_is_always_there_so_the_list_is_never_empty() {
        assert!(installed().contains(&"notepad"));
    }

    #[test]
    fn every_known_app_has_its_own_id() {
        let mut ids: Vec<_> = KNOWN.iter().map(|k| k.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), KNOWN.len());
    }
}
