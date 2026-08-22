# BUILDING.md — <building name>

> The rules of this building. A person writes it; the system evaluates it into a BuildingPolicy. Agents read it and leave it unchanged.

## What this building does

(One paragraph, addressed to an agent arriving for the first time.)

## confidential

`confidential: false`

> Setting this to `true` means: data enters and does not leave, the model pool is restricted to local inference, and writes to other buildings are refused. This is a **structural** decision rather than a switch — it changes what the building is able to do.

## Write domains

(The prefixes residents of this building may write. `.sprawling/` is outside every write domain and is neither listed nor listable here.)

## Reading-room admission

(Which SKILLs this building can read. Name them; a list of names is checkable and "as needed" is not.)

## How work is done here

(Local conventions: naming, commits, review, and what counts as finished. Write the specific ones — "keep quality high" gives an agent nothing to act on.)
