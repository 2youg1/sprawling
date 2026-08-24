# v0.0.1 — pre-alpha

I am sorry for releasing this early. By rights I should have debugged it alone for another week; read this as the impatience that follows a first taste of vibe coding, and accept my apology for the sloppiness.

**This is pre-alpha, and until you are ready to work around problems I do not even want to recommend that you download it.** The harnesses we all use — the ones with tens or hundreds of thousands of stars — have been debugged and tuned for a long time, which is why they rarely go wrong under your hands. You only find out how many details are involved, and how much work a merely *normal* experience costs, after building a harness yourself. The best tool is the one that lets you concentrate on the work instead of on the tool: the one with the least presence, that becomes an extension of the hand, that helps you make something worth making. sprawling has a long way to go before it is that. It is my learning project, and what I want from it is as much experience — and as much criticism — as I can get.

**The front end is bad, and I agree with you about it.** Once I have thought it through I will do what I can to get it right. Future updates will keep trying to make this harness *small and good*; so far only small is true, and I would rather not buy good by giving up small.

**In six months sprawling may matter less.** More and more frontier labs are post-training only against their own harnesses, so the SOTA experience they advertise arrives locked to one ecosystem — open source or not. [RefRain](https://github.com/2youg1/RefRain), which a friend and I are pushing along slowly, exists so that you can keep those official harnesses and still own your editing experience. I will put real effort into it.

**Almost every sentence here still carries a Claudish accent.** I will replace it with better phrasing as chances come, or have the models rewrite all of it once they learn to speak like people.

## The thing nobody else does, as far as I could find

Agents here talk to other running agents freely, **and no budget rations the conversation**. There are good reasons nobody does this, and you can probably name several disasters off the top of your head. My bet is that if models keep getting better at conversation and coordination — without the churn and the spinning-in-place — this may turn out to be worth more as engineering than as a social simulator. It has one extra benefit either way: agents can talk at high frequency, which makes the whole thing feel like a management sim.

Try it with a model trained for multi-agent work — Claude-Opus-5, GPT-5.6-Sol — and it will go better than with one that was not.

And I have a hunch. There are already agents streaming *Slay the Spire 2* runs. So: could four agents take four different roles and clear it as a team? How would they coordinate? What would they do after a loss — hold a post-mortem? Convene a *tribunal*? That is genuinely fascinating, and it is the direction sprawling is designed for. I hope the community builds toward it.

**For anything else, email me. I check often.**

---

## What this release actually contains

Two capabilities landed just before it was cut, both verified end to end rather than by construction.

**Residents can find each other.** A run asks `neighbours` and gets every address it can reach inside its building, each carrying the line that resident's own `URBANITE.md` offers about what to bring them; empty rooms are listed too, because a place to move somebody into is worth knowing about. Detail decays with distance: the rest of the city comes back as building names and nothing more, since reaching another building goes through you. Before this, `signal` took an address the model had to have been told, and a guessed one opened a queue nobody ever read.

**A message reaches its reader whether or not they are working.** Speaking to a resident whose run is going slips the message under the door — it lands at the end of their next tool result. Speaking to one who is idle knocks: the city starts a run for them. Both arrive labelled `@` and the sender's address, which is also the address that answers, and **a resident cannot render as you**: only your own entrance can write the `user` prefix, and that is enforced by a type rather than by discipline. A knock addresses a resident, never a frozen conversation — history is read, not woken.

Two residents in one building negotiated six hours of kiln time to a written agreement over a real provider, each finding the other, arguing a price, and recording what was agreed. That is the evidence behind both claims.

---

## Getting it running

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

## Known before you start

- **Nobody has driven this client in a real browser.** Every capability above was verified through the wire protocol, which is a debugging door rather than the product. The face you will judge this by is the least-tested part of it.
- **First-run has two steps that are easy to miss**: after attaching a provider you must pick a model for `main` *and* for `digest`, or every dispatch is refused with `no model is chosen for this tag`.
- **`exec` has three arms and two of them need something installed.** The shell arm needs a shell interpreter and the Python arm needs a CPython-WASI component; without them both answer `E_TOOL_UNAVAILABLE`, while the third arm runs a program on your machine normally.
