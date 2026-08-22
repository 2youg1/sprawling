# Runtime document templates

Once a city is running, agents and the person share a set of markdown files. Their formats live in this directory.

**A template teaches by being present, and a template is a form to fill in rather than a question to answer.** "How to write this well" is carried by the **structure** of the file rather than by instruction in the prefix: reconciliation can only judge a line after it is written, while a form makes a small local model write it correctly the first time. That matters most for confidential work, which only local models are allowed to do.

Templates land in the building root with `CreateBuilding`. A resident opens the file and sees the skeleton.

| File | Location | Written by | What it is |
|---|---|---|---|
| `Roadmap.md` | building root | agent | The single denominator for plan and progress. A city has no second roadmap and no todo tool. |
| `Memo.md` | building root | agent | Decisions and corrections. The outline is rewritten in place; the body is append-only. |
| `Handoff.md` | building root | agent | The recovery package, five sections. **Not a new authority.** |
| `BUILDING.md` | building root | person | The rules of the building. Evaluated into a BuildingPolicy. |
| `JOB.md` | room | person or dispatcher | The task for this session. The agent **reads it and leaves it unchanged**. |
| `URBANITE.md` | with the resident | person | Who this resident is and how they work. |

The four files in the building root are together called the **Spine**. Long-running work stays continuous through these files rather than through session memory — this is what "no continuous self" looks like at the file layer.

**The Roadmap table is parsed** (`kernel::spine`): four columns, five status words, and an evidence column that holds a Locator. A row that does not parse is displayed and excluded from the completion figure, so a table can never quietly inflate its own progress.
