//! Application-menu integration: installs a freedesktop `.desktop` entry and
//! the app icon for the current user (`~/.local/share`).
//!
//! On Wayland the window/taskbar icon comes from this entry (matched by the
//! app id `headphones-control`), so without it there is no logo anywhere.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const APP_ID: &str = "headphones-control";

const ICON_SVG: &[u8] = include_bytes!("../assets/headphones-control.svg");
const ICON_PNG: &[u8] = include_bytes!("../assets/headphones-control-256.png");

/// `$XDG_DATA_HOME`, or `~/.local/share`.
pub fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
}

fn entry_path(data: &Path) -> PathBuf {
    data.join("applications").join(format!("{APP_ID}.desktop"))
}

fn icon_paths(data: &Path) -> [PathBuf; 2] {
    let hicolor = data.join("icons/hicolor");
    [
        hicolor.join(format!("scalable/apps/{APP_ID}.svg")),
        hicolor.join(format!("256x256/apps/{APP_ID}.png")),
    ]
}

/// Is a menu entry installed (by this app, install.sh or a package)?
pub fn is_installed() -> bool {
    let system = ["/usr/share/applications", "/usr/local/share/applications"]
        .iter()
        .any(|d| Path::new(d).join(format!("{APP_ID}.desktop")).exists());
    system || data_home().is_some_and(|d| entry_path(&d).exists())
}

/// Quote a path for the `Exec=` key (Desktop Entry Specification).
fn quote_exec(path: &str) -> String {
    let mut out = String::from("\"");
    for c in path.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    // In the .desktop file format itself, a backslash must be doubled again.
    out.replace('\\', "\\\\")
}

pub fn entry_text(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Headphones Control\n\
         GenericName=Headphones Settings\n\
         Comment=Noise cancelling, ambient sound and equalizer for Sony headphones (unofficial)\n\
         Exec={}\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         Categories=AudioVideo;Audio;Settings;HardwareSettings;\n\
         Keywords=sony;headphones;noise;cancelling;anc;ambient;equalizer;bluetooth;\n\
         StartupWMClass={APP_ID}\n",
        quote_exec(&exe.to_string_lossy())
    )
}

/// Install the entry and icons under `data` (normally [`data_home`]),
/// launching `exe`.
pub fn install_into(data: &Path, exe: &Path) -> io::Result<()> {
    let entry = entry_path(data);
    if let Some(dir) = entry.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&entry, entry_text(exe))?;
    let [svg, png] = icon_paths(data);
    for (path, bytes) in [(svg, ICON_SVG), (png, ICON_PNG)] {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, bytes)?;
    }
    Ok(())
}

pub fn uninstall_from(data: &Path) -> io::Result<()> {
    let [svg, png] = icon_paths(data);
    for path in [entry_path(data), svg, png] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Install for the current user, launching the running executable.
pub fn install() -> io::Result<()> {
    let data =
        data_home().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home directory"))?;
    let exe = std::env::current_exe()?;
    install_into(&data, &exe)?;
    refresh_menus(&data);
    Ok(())
}

pub fn uninstall() -> io::Result<()> {
    let data =
        data_home().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home directory"))?;
    uninstall_from(&data)?;
    refresh_menus(&data);
    Ok(())
}

/// Ask desktops to notice the change (KDE's menu cache, icon caches).
/// Missing tools are fine: most desktops watch these folders anyway.
fn refresh_menus(data: &Path) {
    let apps = data.join("applications");
    let icons = data.join("icons/hicolor");
    let commands: [(&str, Vec<&std::ffi::OsStr>); 4] = [
        ("kbuildsycoca6", vec![]),
        ("kbuildsycoca5", vec![]),
        ("update-desktop-database", vec![apps.as_os_str()]),
        (
            "gtk-update-icon-cache",
            vec!["-q".as_ref(), "-t".as_ref(), icons.as_os_str()],
        ),
    ];
    for (cmd, args) in commands {
        let _ = std::process::Command::new(cmd)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_paths_are_quoted() {
        assert_eq!(quote_exec("/home/me/bin/app"), "\"/home/me/bin/app\"");
        assert_eq!(quote_exec("/home/my dir/app"), "\"/home/my dir/app\"");
        // $ and " are escaped with a backslash, which the file format doubles.
        assert_eq!(quote_exec("/a$b\"c"), "\"/a\\\\$b\\\\\"c\"");
    }

    #[test]
    fn install_and_uninstall() {
        let dir = std::env::temp_dir().join(format!("hpc-desktop-test-{}", std::process::id()));
        install_into(&dir, Path::new("/opt/hc/headphones-control")).unwrap();
        let text = fs::read_to_string(entry_path(&dir)).unwrap();
        assert!(text.contains("Exec=\"/opt/hc/headphones-control\"\n"));
        assert!(text.contains("Icon=headphones-control\n"));
        assert!(icon_paths(&dir).iter().all(|p| p.exists()));
        uninstall_from(&dir).unwrap();
        assert!(!entry_path(&dir).exists());
        uninstall_from(&dir).unwrap(); // idempotent
        let _ = fs::remove_dir_all(&dir);
    }
}
