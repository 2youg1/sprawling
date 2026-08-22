# sprawling — start here

You have unpacked a folder. Nothing was installed, no service was registered, and nothing outside this folder is written to. Deleting the folder removes everything.

## 1 Start it

- **Windows** — double-click `start.cmd`.
- **macOS and Linux** — run `./start.sh` from a terminal.

The first time on Windows you will see **"Windows protected your PC"**, because this program carries no code-signing certificate. Choose **More info**, then **Run anyway**.

A console window opens and stays open. **That window is the city** — closing it, or pressing `Ctrl-C` in it, stops the city. Your browser opens at <http://127.0.0.1:8787>; if it does not, open that address yourself.

The first screen asks one question before it creates anything, and shows you the folder it is about to create.

## 2 Give it a model to think with

This program schedules agents, records what they do, and shows it to you. **It does not think by itself**, so before it can do anything you need one of:

- an API key from a provider that speaks the OpenAI dialect or the Anthropic dialect, or
- a subscription login (Anthropic today).

## 3 Four steps in the page

1. **settings** — the provider's base URL, its dialect, and the key. The key goes straight into your operating system's credential service; the page only ever shows a `secret:realm/name` reference afterwards.
2. Same page — choose a model for `main` (the one that thinks) and for `digest` (the one that reads long documents on its behalf). They may be the same model.
3. **city** — raise a building, for example `lab`.
4. The bar at the bottom of every page — an address, what to produce, and what counts as done. Press **send it**.

Then **live** follows the work, **approvals** holds anything that needs you, and **cost** is what it spent.

## Where your data is

In `city/`, beside this file. One folder holds the whole history, and it can be moved, copied or deleted as a unit.

## Everything else

Run `sprawling help` for the full command list. The complete walkthrough, the vocabulary, and the design are at <https://github.com/2youg1/sprawling>.
