**Download the archive for your system, unpack it, and run the launcher inside.** Nothing is installed and nothing outside that folder is written to.

| System | Archive | What to run |
|---|---|---|
| Windows | `sprawling-*-windows-x86_64.zip` | double-click `start.cmd` |
| macOS | `sprawling-*-macos-aarch64.zip` | `./start.sh` in a terminal |
| Linux | `sprawling-*-linux-x86_64.zip` | `./start.sh` in a terminal |

A console window opens and stays open — **that window is the city**. Your browser opens at <http://127.0.0.1:8787>. `Ctrl-C` in the window stops the city.

**These binaries are not code-signed.** Windows will say *"Windows protected your PC"*: choose **More info → Run anyway**. macOS will refuse the first run: open it once from Finder's right-click menu, or clear the quarantine attribute.

**Before it can do anything you need a model to call** — an API key for a provider speaking the OpenAI or Anthropic dialect, or a subscription login. This program schedules agents and records what they do; it does not think by itself.

`QUICKSTART.md` inside the archive walks the first ten minutes. Every archive also carries `sbom.cdx.json`, the full bill of materials for the binary beside it.
