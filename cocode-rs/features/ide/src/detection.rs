//! IDE type detection and registry.
//!
//! Defines supported IDE types and provides a registry matching
//! Claude Code's IDE configuration map.

/// IDE family classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdeKind {
    /// VS Code and derivatives (Cursor, Windsurf, VSCodium).
    VsCode,
    /// JetBrains IDEs (IntelliJ, PyCharm, WebStorm, etc.).
    JetBrains,
}

impl IdeKind {
    /// User-facing term for the IDE's add-on package.
    pub fn addon_term(self) -> &'static str {
        match self {
            IdeKind::VsCode => "extension",
            IdeKind::JetBrains => "plugin",
        }
    }
}

/// Describes a specific IDE product.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeType {
    /// Short key used in lockfiles and config (e.g. "vscode", "cursor").
    pub key: &'static str,
    /// IDE family.
    pub kind: IdeKind,
    /// Human-readable name (e.g. "VS Code", "IntelliJ IDEA").
    pub display_name: &'static str,
    /// Process name patterns used for detection on Linux.
    pub linux_process: &'static [&'static str],
    /// Process name patterns used for detection on macOS.
    pub macos_process: &'static [&'static str],
    /// Process name patterns used for detection on Windows.
    pub windows_process: &'static [&'static str],
}

/// Look up an IDE type by its lockfile key.
pub fn ide_for_key(key: &str) -> Option<&'static IdeType> {
    IDE_REGISTRY.iter().find(|ide| ide.key == key)
}

/// Full registry of supported IDEs (4 VS Code + 15 JetBrains = 19 types),
/// matching Claude Code's `gX6` map.
pub static IDE_REGISTRY: &[IdeType] = &[
    // VS Code family
    IdeType {
        key: "vscode",
        kind: IdeKind::VsCode,
        display_name: "VS Code",
        linux_process: &["code"],
        macos_process: &["Visual Studio Code.app", "Code Helper"],
        windows_process: &["Code.exe"],
    },
    IdeType {
        key: "cursor",
        kind: IdeKind::VsCode,
        display_name: "Cursor",
        linux_process: &["cursor"],
        macos_process: &["Cursor.app", "Cursor Helper"],
        windows_process: &["Cursor.exe"],
    },
    IdeType {
        key: "windsurf",
        kind: IdeKind::VsCode,
        display_name: "Windsurf",
        linux_process: &["windsurf"],
        macos_process: &["Windsurf.app", "Windsurf Helper"],
        windows_process: &["Windsurf.exe"],
    },
    IdeType {
        key: "vscodium",
        kind: IdeKind::VsCode,
        display_name: "VSCodium",
        linux_process: &["codium"],
        macos_process: &["VSCodium.app"],
        windows_process: &["VSCodium.exe"],
    },
    // JetBrains family
    IdeType {
        key: "intellij",
        kind: IdeKind::JetBrains,
        display_name: "IntelliJ IDEA",
        linux_process: &["idea"],
        macos_process: &["IntelliJ IDEA"],
        windows_process: &["idea64.exe"],
    },
    IdeType {
        key: "pycharm",
        kind: IdeKind::JetBrains,
        display_name: "PyCharm",
        linux_process: &["pycharm"],
        macos_process: &["PyCharm"],
        windows_process: &["pycharm64.exe"],
    },
    IdeType {
        key: "webstorm",
        kind: IdeKind::JetBrains,
        display_name: "WebStorm",
        linux_process: &["webstorm"],
        macos_process: &["WebStorm"],
        windows_process: &["webstorm64.exe"],
    },
    IdeType {
        key: "phpstorm",
        kind: IdeKind::JetBrains,
        display_name: "PhpStorm",
        linux_process: &["phpstorm"],
        macos_process: &["PhpStorm"],
        windows_process: &["phpstorm64.exe"],
    },
    IdeType {
        key: "rubymine",
        kind: IdeKind::JetBrains,
        display_name: "RubyMine",
        linux_process: &["rubymine"],
        macos_process: &["RubyMine"],
        windows_process: &["rubymine64.exe"],
    },
    IdeType {
        key: "clion",
        kind: IdeKind::JetBrains,
        display_name: "CLion",
        linux_process: &["clion"],
        macos_process: &["CLion"],
        windows_process: &["clion64.exe"],
    },
    IdeType {
        key: "goland",
        kind: IdeKind::JetBrains,
        display_name: "GoLand",
        linux_process: &["goland"],
        macos_process: &["GoLand"],
        windows_process: &["goland64.exe"],
    },
    IdeType {
        key: "rider",
        kind: IdeKind::JetBrains,
        display_name: "Rider",
        linux_process: &["rider"],
        macos_process: &["Rider"],
        windows_process: &["rider64.exe"],
    },
    IdeType {
        key: "datagrip",
        kind: IdeKind::JetBrains,
        display_name: "DataGrip",
        linux_process: &["datagrip"],
        macos_process: &["DataGrip"],
        windows_process: &["datagrip64.exe"],
    },
    IdeType {
        key: "appcode",
        kind: IdeKind::JetBrains,
        display_name: "AppCode",
        linux_process: &["appcode"],
        macos_process: &["AppCode"],
        windows_process: &["appcode64.exe"],
    },
    IdeType {
        key: "dataspell",
        kind: IdeKind::JetBrains,
        display_name: "DataSpell",
        linux_process: &["dataspell"],
        macos_process: &["DataSpell"],
        windows_process: &["dataspell64.exe"],
    },
    IdeType {
        key: "aqua",
        kind: IdeKind::JetBrains,
        display_name: "Aqua",
        linux_process: &["aqua"],
        macos_process: &["Aqua"],
        windows_process: &["aqua64.exe"],
    },
    IdeType {
        key: "gateway",
        kind: IdeKind::JetBrains,
        display_name: "Gateway",
        linux_process: &["gateway"],
        macos_process: &["Gateway"],
        windows_process: &["gateway64.exe"],
    },
    IdeType {
        key: "fleet",
        kind: IdeKind::JetBrains,
        display_name: "Fleet",
        linux_process: &["fleet"],
        macos_process: &["Fleet"],
        windows_process: &["Fleet.exe"],
    },
    IdeType {
        key: "androidstudio",
        kind: IdeKind::JetBrains,
        display_name: "Android Studio",
        linux_process: &["studio"],
        macos_process: &["Android Studio"],
        windows_process: &["studio64.exe"],
    },
];

#[cfg(test)]
#[path = "detection.test.rs"]
mod tests;
