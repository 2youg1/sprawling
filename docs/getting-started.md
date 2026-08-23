# Getting started — from an empty directory to one finished run

> **For someone installing this for the first time.** It walks the whole path once, skipping no step, and stops when a model has done a piece of work and written a file.
>
> It does not explain the vocabulary ([`glossary.md`](glossary.md)), day-to-day operation ([`operating.md`](operating.md)), or the design ([`../ARCHITECTURE.md`](../ARCHITECTURE.md)). 中文版：[`getting-started.zh-CN.md`](getting-started.zh-CN.md).

## What you need

A desktop browser and the `sprawling` binary. Nothing else — no npm, no node, no language runtime, no database.

The quickest way to both: take the archive for your system from the [latest release](../../../releases/latest), unpack it, and run the binary with no arguments — double-click `sprawling.exe` on Windows, `./sprawling` on macOS and Linux. With no command it asks whether to start a city here, showing the folder it is about to create, and then does steps 1 and 2 below in one go and opens the page for you; come back here at step 3. To make `sprawling` a word your shell resolves, run `sprawling install` once. The rest of this walkthrough uses the commands, because knowing them is what lets you run a second city, move one between machines, or drive it from a script.

You also need one model to call. Either an API key for a provider that speaks the OpenAI or the Anthropic dialect, or a local server that speaks one of them.

To build the binary yourself rather than take one, see [`CONTRIBUTING.md`](CONTRIBUTING.md); the front end is built first and embedded into the binary, so a `cargo build` alone produces a binary whose page is out of date.

## 1 Raise a city

```bash
sprawling init ~/cities/first
```

A city is one directory and everything is inside it. This writes the genesis record, which is also where the city's name is kept — the browser reads it from there rather than from the directory name.

## 2 Start the control surface

```bash
sprawling serve ~/cities/first
```

It binds `127.0.0.1:8787` and prints where it is listening. Open that address, or add `--open` and let it open the browser once the port answers.

Steps 1 and 2 together, for a city you have not raised yet, are one command — it is what the launcher in the release archive runs, and with no directory given it puts the city beside the binary:

```bash
sprawling up ~/cities/first
```

To reach it from another machine on your network, give an address to bind and set a pairing token first:

```bash
SPRAWLING_PAIRING_TOKEN=<a token you choose> sprawling serve ~/cities/first 0.0.0.0:8787
```

Binding a non-loopback address with no token **refuses to start**. That is a binding decision rather than a request-time one: a city that starts and then rejects everyone looks like a network fault, and a city that starts and accepts everyone is worse.

## 3 Give it a provider

Open **settings** in the left nav, under *setup*.

Fill in the name you will call it by, the base URL, and the dialect. The URL rule is stated where you type it: `https` anywhere, `http` only to this machine. Paste the key last.

The key never becomes part of a command. It goes to the credential service of your operating system through its own route, and what comes back to the page is a reference of the form `secret:realm/name`. From then on that reference is what appears in configuration, in events, and in logs; the plaintext is in the vault and nowhere else. The input box is cleared as soon as it is sent.

Press **attach**. The city calls the provider's model list to find out what it offers. A provider that answers nothing usable says so here rather than at the first dispatch.

Then choose a model per tag. Two tags matter at the start:

- `main` — the model that thinks.
- `digest` — the model that reads long documents on `main`'s behalf.

They may be the same model. The context window and output limit are not asked for: those are facts about the model, and the city reads them from the catalogue it just fetched.

### Or sign in with a subscription

If your provider is one you pay for by subscription rather than by token, use **sign in with a subscription** on the same page. It begins the login, shows you a URL to open, and waits for the code the provider displays. Paste that code back and the token lands in the credential service with the endpoint attached behind it, renewed before it expires rather than after a call comes back refused.

Only the Anthropic row of that intelligence table is complete today. A provider whose row is empty refuses to begin rather than sending you to an empty page.

## 4 Raise a building

Go to **city**. The form under the drawing takes a name and a template.

- `minimal` — an ordinary building.
- `confidential` — data may enter and may not leave: the model pool is local, and writes to other buildings are always refused.

A building name is a top-level address: `lab`, not `lab/room1`. Rooms come later and they are just directories.

The new building gets its own documents immediately — `Roadmap.md`, `Memo.md` and `Handoff.md` at its root, which are the building's memory and which agents both read and write, plus `.sprawling/BUILDING.md`, which is its rules: you write that one and the agents working there can only read it. You can read them in the browser by selecting the building and pressing **read it**; the same files are on disk if you would rather use an editor.

## 5 Dispatch one piece of work

The bar at the bottom of every page is the control surface. It asks four things:

| Field | What it wants |
|---|---|
| address | which room, as `building/room` — or just the building |
| what to produce | one line |
| what counts as done | the condition you will judge it by |
| mode | how it should work; `plan` is the ordinary one |

It does not ask for a budget. Nobody can say what a piece of work is worth before it runs, and a subscription has no unit price at all; what it cost is reported afterwards, from the record, on the cost page.

Press **send it**. The city writes `run_started`, assembles the frozen prefix, calls the model, and from then on every tool call and every result is a line in the Ledger.

## 6 Watch it, and read what it did

**live** follows the session. Pick which run you are watching — with two in flight, "the latest one" is a coin toss, so the page makes you choose rather than choosing for you. The window is bounded and says how many lines fell out of it; those lines are still in the Ledger.

**ledger** is the whole stream, filtered and paged. It only shows what arrived after this page connected, and it says so; the city's total length is on the vital signs strip at the top of the city page.

**cost** is what it spent, cut five ways. Where the provider reported no price, the page reports tokens and says why there is no amount instead of printing `$0.00`.

When the run finishes, the file it wrote is in the building. Open the building page to read it, or look on disk — they are the same bytes.

## 7 Stop, and start again

`Ctrl-C` on the server, or **stop the city** in the page, which halts every run and is recorded.

After a restart:

```bash
sprawling resume ~/cities/first
```

That verifies the chain, closes tool calls whose outcome was lost when the process died, and reports what is waiting for a person. A city that was killed mid-call comes back saying what it does not know rather than pretending.

## If something does not work

| What you see | What it means |
|---|---|
| The page says the schema does not match | The binary and the page in your browser are different versions. Reload. |
| The model list is empty after attaching | The provider answered, but with nothing this build could read. The refusal names what it got. |
| A run stops and asks for something | Something needs a person. It is on the **approvals** page, grouped so one answer covers one question. |
| A run froze | Budget, watchdog, or a limit. The live page says which. |
| A file is missing | **recycle bin**. Every row states how to get that file back. |

More of these, with what to do about each, are in [`operating.md`](operating.md).
