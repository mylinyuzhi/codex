Use this skill when the user asks about keyboard shortcuts, rebinding keys, adding chord bindings, or modifying their keybindings configuration.

## Keybinding Configuration

Users customize keybindings via `~/.cocode/keybindings.json`:

```json
{
  "bindings": [
    {
      "context": "Chat",
      "bindings": {
        "ctrl+k ctrl+c": "ext:clearScreen",
        "meta+p": "chat:modelPicker",
        "ctrl+z": null
      }
    }
  ]
}
```

- **Chord bindings**: Space-separated keystrokes (e.g., `ctrl+k ctrl+c`), 1-second timeout
- **Unbinding**: Set action to `null` to remove a default binding
- **Command bindings**: `command:<slash-command-name>` executes a skill
- **Merge semantics**: User bindings are additive; last-match-wins for duplicate keys

## Available Contexts (18)

Global, Chat, Autocomplete, Confirmation, Help, Transcript, HistorySearch, Task, ThemePicker, Settings, Tabs, Attachments, Footer, MessageSelector, DiffDialog, ModelPicker, Select, Plugin

## Default Keyboard Shortcuts

### Global (always active)
| Key | Action |
|-----|--------|
| Ctrl+C | Interrupt |
| Ctrl+R | History search |
| Ctrl+V / Alt+V | Paste (image or text) |
| PageUp / PageDown | Scroll chat |

### Chat (primary input context)
| Key | Action |
|-----|--------|
| Enter / Ctrl+Enter | Submit input (queue if streaming) |
| Shift+Enter / Alt+Enter | Insert newline |
| Tab | Toggle plan mode |
| Esc | Cancel / close |
| Esc Esc | Open rewind selector (chord) |
| Ctrl+T | Cycle thinking level |
| Ctrl+Shift+T | Toggle thinking display |
| Ctrl+M | Cycle model |
| Ctrl+B | Background all tasks |
| Ctrl+F | Kill running agents |
| Ctrl+E | Open external editor |
| Ctrl+G | Open plan file in editor |
| Ctrl+P | Command palette |
| Ctrl+S | Session browser |
| Ctrl+L | Clear screen |
| Ctrl+Q | Quit |
| Ctrl+A | Select all |
| Ctrl+Shift+E | Toggle tool collapse |
| Ctrl+Shift+R | Toggle system reminders |
| ? / F1 | Show help |
| Ctrl+K | Kill to end of line |
| Ctrl+Y | Yank (paste kill buffer) |
| Left/Right/Up/Down | Cursor movement |
| Home / End | Line start / end |
| Ctrl+Left/Right | Word navigation |
| Backspace / Delete | Delete character |
| Ctrl+Backspace/Delete | Delete word |
| Alt+Up/Down | Scroll chat |
| Ctrl+Up/Down | Page up/down |

### Autocomplete (suggestions visible)
| Key | Action |
|-----|--------|
| Up/Down | Navigate suggestions |
| Tab / Enter | Accept suggestion |
| Esc | Dismiss |

### Confirmation (permission/approval dialogs)
| Key | Action |
|-----|--------|
| Y / N | Yes / No |
| Ctrl+A | Approve all |
| Up/Down, K/J | Navigate |
| Enter | Confirm |
| Esc / Ctrl+C | Cancel |

### Task (agent running)
| Key | Action |
|-----|--------|
| Ctrl+B | Background task |

### MessageSelector (rewind checkpoint browser)
| Key | Action |
|-----|--------|
| Up/Down, K/J | Navigate |
| Ctrl+Up/Down, Shift+Up/Down | Jump to top/bottom |
| Shift+K/J | Jump to top/bottom |
| Enter | Select |
| Esc | Cancel |

### HistorySearch (Ctrl+R search)
| Key | Action |
|-----|--------|
| Ctrl+R | Next match |
| Esc / Tab | Accept |
| Ctrl+C | Cancel |
| Enter | Execute |

### DiffDialog
| Key | Action |
|-----|--------|
| Esc | Dismiss |
| Left/Right | Navigate sources |
| Up/Down | Navigate files |
| Enter | View details |

### Transcript
| Key | Action |
|-----|--------|
| Ctrl+E | Toggle show all |
| Ctrl+C / Esc | Exit |

### Help / ModelPicker / Select / Plugin / Theme
Standard navigation: Up/Down to navigate, Enter to accept, Esc to cancel. Plugin overlay supports Tab/Shift+Tab for tab switching.

## Modifier Keys

Modifiers: `ctrl`, `alt`/`opt`/`option`, `shift`, `meta`/`cmd`/`command`/`super`/`win`

Special keys: `enter`, `escape`/`esc`, `tab`, `backspace`, `delete`, `up`, `down`, `left`, `right`, `pageup`, `pagedown`, `home`, `end`, `space`, `f1`-`f12`

## Reserved Keys

- `Ctrl+C` — terminal interrupt (SIGINT)
- `Ctrl+Z` — Unix process suspend (SIGTSTP)
- `Ctrl+D` — terminal EOF
- `Ctrl+M` — identical to Enter in terminals
- `Ctrl+\` — terminal quit signal (SIGQUIT)
- macOS: `Cmd+C/V/X/Q/W/Tab/Space` intercepted by OS
