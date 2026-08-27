# v0.0.2 — pre-alpha

*中文版在后半篇。*

**This replaces the v0.0.2 I put out this morning.** Same version number on purpose: nothing here changes what the program does, only whether you can watch it do it. I would rather correct a release than let a number tell you the front end was finished.

Everything the v0.0.1 notes said about being pre-alpha still holds, and I will not repeat all of it: this is my learning project, and I would rather have your criticism than your patience.

Last time I wrote that the front end was bad and that I agreed with you about it. I have spent this release on that, and I want to be exact about what changed, because "improved the UI" is the kind of sentence that means nothing.

## The interface was hiding the product

That is the honest summary. Everything sprawling does that other harnesses do not, it does **inside a single turn** — a refusal that tells you what it refused and how to get past it, a checkpoint fence, a write stopped at the edge of its domain, a compaction that says what it dropped. The client folded three of fifty-eight kinds of event and threw the rest away, so all of that reached your screen as the same grey line as a successful file read. You could not see the thing I built.

**A turn now tells you what happened in it.** What the model said, what the turn cost in tokens and in dollars, what each tool actually answered, and what a door refused along with the way round it. What stings is that **none of this was new data** — the payload had been carrying the message, the usage, the stop reason and the billed amount the whole time, and the fold simply never read them. I had also written in a note to myself that token counts were "not on the wire". They were. I was wrong, and I have said so in the record.

**Opening yesterday's session showed you an empty page.** There was one history query and it did not take a session, so four sessions running at once split a single slice of five hundred records between them, and anything older than that slice was not in it at all. You can now ask for one session. The tab still holds a bounded amount of history — it has to, or a tab left open all night dies — but it now gives way in the session you are *not* reading rather than in the one you are.

**Every tool wave has been fenced by a real git commit since early on, and nothing could read one back.** The write side was git-native and the read side did not exist. A session now lists the files it changed with `+` and `−` counts, taken from git between two real checkpoints. One thing I am quietly pleased about: because the fence is the write domain, that list *cannot* contain a file an agent merely read — which is exactly the complaint people have about the same feature elsewhere, and here it comes free from a decision made for another reason entirely.

**Dragging a file onto the box where you write work did nothing** — and worse, the browser was quietly answering the gesture instead. A plain text input accepts a dropped selection all by itself, so a drag went in raw and every rule I had written about what a drop means never ran. A drop now reaches the composer and a running session's own box; it fills the box and **never presses the button**, because a gesture that spent money would be one you could not take back. And a drop target now lights up while you are dragging over it, which the old hover rule could never do — a browser suppresses pointer events for the whole of a drag, so that outline was invisible at exactly the moment you needed it.

**A building's own pages were one flat wall of text.** Headings, lists, links, inline code and fenced blocks now come apart. They come apart by lightness, by weight, by slant — and **by no colour at all**. This interface has exactly two colours: one means "something is happening here", the other means "a person is needed here". A syntax-coloured document would have spent both of them on something that means neither. Once you notice that constraint it stops feeling like a constraint.

All of it cost 35 KB in the client, weighed one change at a time. The budget is 2 MB and this uses under a third of it.

## Three features that existed and could not be reached

These are from the first cut of v0.0.2 and still describe it. Each was found the same way: by asking who calls a thing, and finding nobody.

**A city could not accept a browser from another machine.** Exposing the WebUI beyond loopback is a four-link chain and three links were cut. `PairingToken::mint` had no caller — a type whose own documentation says it hands back a code "to show to a person once" had never shown one. The page built its link without the token, and the socket URL kept only the host, dropping the query string on the way. So a city bound to `0.0.0.0` with `SPRAWLING_PAIRING_TOKEN` set demanded a token from every peer while its own client never sent one: **the server refused its own WebUI, in the exact configuration the token exists to enable.**

**The console could not answer a question.** It replied to every query with the text of a `sprawling call` command — asking someone already standing inside a city to open a second terminal and interrogate it from outside. The function that could answer had been built one line above and handed only to the socket.

**`exec`'s refusal named an install nobody could perform.** The refusal reads *"use the program arm, or install a build with the `wasm` feature"*, and **that build did not exist**. It exists now, behind `--features sandbox`. Read the last section before you expect it in this archive.

## Four failures that were reported as something else

**A run under review put its decisions on the building's shelf.** A building under review lends every run its own tree so that nothing it writes belongs to the building until somebody checks it. The archive drain wrote outside that tree.

**A file that would not open looked like a file somebody wrote badly.** An unreadable plan was flattened to an empty string and then reported as a plan somebody had changed; an unreadable handoff meant "there was no handoff". Both now say what actually happened — and that difference matters most where it is least visible.

**A change could outrun its own record.** A line the history refused is now a change the city never made, and a merge waits for the line that announces it.

**A ceiling could be lost twice.** Work handed to another resident now runs under the ceiling that sent it, and answering an approval resumes the same work instead of restarting it under a default.

**The same ask arriving twice is one piece of work.** Retries and duplicate frames now settle against the idempotency key the command already carried, instead of one minted from content.

## Under the surface

The dispatch path went from a single 1069-line function to 158 across named phases. A machine gate now fails the build on any production function past 200 lines, and nothing was exempted to make it pass. There are 1,206 tests and thirteen gates, and a change is not finished here until all of them are green.

---

## Getting it running

**Download the archive for your system, unpack it, and run the launcher inside.** Nothing is installed and nothing outside that folder is written to.

| System | Archive | What to run |
|---|---|---|
| Windows | `sprawling-0.0.2-windows-x86_64.zip` | double-click `start.cmd` |
| macOS | `sprawling-0.0.2-macos-aarch64.zip` | `./start.sh` in a terminal |

A console window opens and stays open — **that window is the city**. Your browser opens at <http://127.0.0.1:8787>. `Ctrl-C` in the window stops the city.

**These binaries are not code-signed.** Windows will say *"Windows protected your PC"*: choose **More info → Run anyway**. macOS will refuse the first run: open it once from Finder's right-click menu, or clear the quarantine attribute.

**Before it can do anything you need a model to call** — an API key for a provider speaking the OpenAI or Anthropic dialect, or a subscription login. This program schedules agents and records what they do; it does not think by itself.

`QUICKSTART.md` inside the archive walks the first ten minutes. Every archive also carries `sbom.cdx.json`, the full bill of materials for the binary beside it.

## Please know this before you start

- **No real session has ever gone through this interface.** Every screen I described above is asserted against records built inside a test and rendered without a browser. 1,206 tests and thirteen gates say the folds, the refusals and the change list are right about the data they are handed. What no gate in this repository can do is attach a provider, dispatch real work, and *look*. **So the turn view, the change list and the document reader have never shown a single byte that a model produced.** Please read that sentence before you judge how any of it looks — and if it looks wrong when you try it, that is the most useful thing you could tell me. The keyboard and the command palette are the exception; those keys were pressed in a real browser.

- **This archive does not carry the execution engine.** `sandbox` is off by default because wasmtime is a large binary and I did not reopen that trade here. Anything routed through the sandbox still answers `this build carries no execution engine`; the arm that runs a program on your machine works as before. Build it yourself with `cargo build --release -p sprawling --features sandbox`.

- **No Linux archive in this release.** The Linux pipeline was ruled out rather than debugged. Windows and macOS are what ships.

- **First run has two steps that are easy to miss**: after attaching a provider you must pick a model for `main` *and* for `digest`, or every dispatch is refused with `no model is chosen for this tag`.

- **Reaching a city from another machine needs `SPRAWLING_PAIRING_TOKEN`.** Binding to a non-loopback address without one is refused on purpose. With one set, the console prints a URL carrying the key — open *that* URL rather than typing one by hand, or the server will refuse you the way it used to refuse itself.

**For anything else, email me. I check often.**

---
---

# v0.0.2 — pre-alpha（中文）

**这一版覆盖了我今天上午发的 v0.0.2。** 版本号是故意不动的:这里没有一处改变程序做什么,只改变你能不能看见它在做。与其让一个新号码暗示前端做完了,我宁愿把同一个号码重发一次。

v0.0.1 说过的关于 pre-alpha 的话现在仍然成立,我不再重复一遍:这是我的学习项目,比起你的耐心,我更想要你的批评。

上次我写过前端很糟,而且我同意你的看法。这一版我花在这件事上,并且想把改了什么说准确——因为「优化了 UI」这种句子等于什么都没说。

## 界面一直在挡着这个产品

这是实话。sprawling 与别的 harness 不一样的地方,全部发生在**一轮里面**——一次说得出拒了什么、怎么绕过去的拒绝;一道检查点栅栏;一次被写域拦下的写入;一次报得出自己丢了什么的压缩。而客户端只折叠了 58 种事件里的 3 种,其余全扔。于是这些东西到你屏幕上,和一次成功的读文件长成同一行灰字。你看不见我造的那个东西。

**现在一轮说得出自己发生过什么。** 模型说了什么,这一轮花了多少 token、多少钱,每个工具到底回了什么,以及哪道门拒了什么、怎么绕过去。扎心的是**这些数据一个都不是新的**——载荷里一直带着模型的话、用量、停止原因和账单金额,只是折叠函数从来没读过它们。我甚至在自己的笔记里写过「token 用量线上没有,是我编的」。它有。我记错了,并且已经把这条更正写进记录。

**打开昨天的会话,给你的是一张空白页。** 历史只有一个查询,而它不带会话;于是四个同时在跑的会话分同一片 500 条的切片,比这片切片更早的东西根本不在里面。现在可以只问一个会话。标签页仍然只持有有限的历史——必须有限,否则一个开一整夜的标签页会死——但它现在让位的是你**没在看**的那个会话,而不是你正在看的这个。

**每一次工具浪早就有一个真的 git commit 给它围栅栏,而没有任何地方读得回来。** 写入侧是 git 原生的,读出侧根本不存在。现在一个会话会列出它改过的文件,带 `+` 和 `−`,数字取自两个真检查点之间的 git 差异。有一件事我挺得意:因为栅栏就是写域,这张表**不可能**混进 agent 只是读过的文件——而这恰恰是别处同类功能被人抱怨的地方,在这里它是另一个决定的免费副产品。

**把文件拖到写活的那个框里,什么也不会发生**——更糟的是,浏览器在背地里替这个手势作答。一个普通文本输入框自己就接受被拖进来的文本,于是拖进去的东西是原样插入的,而我为「拖进来意味着什么」写的所有规则一行都没跑。现在拖拽能落到派活条,也能落到一个正在跑的会话自己的框里;它**只把字写进框,绝不替你按按钮**——一个会花钱的手势,是你收不回来的手势。而且投放区现在会在你拖着东西经过时亮起来,这是旧的 hover 规则永远做不到的:浏览器在拖拽全程屏蔽指针事件,所以那道虚线恰好在你最需要它的那一刻是隐形的。

**一栋楼自己的文档,过去是一整面平的字墙。** 标题、列表、链接、行内代码、围栏代码块现在分得开了。它们靠明度、字重、斜体分开,**并且完全不靠颜色**。这个界面总共只有两种颜色:一种意思是「这里正在发生什么」,另一种是「这里需要人」。一份五彩的文档会把这两种颜色都花在既不是前者也不是后者的东西上。这个约束一旦看明白,就不再像约束了。

以上全部在客户端上花了 35 KB,一次改动一次称重。预算是 2 MB,现在用掉不到三分之一。

## 三个已经存在、却够不着的功能

这些来自 v0.0.2 的第一次发布,现在依然成立。每一个都是用同一种方式找到的:问一句「谁在调用它」,然后发现没有人。

**一座城接不了另一台机器上的浏览器。** 把 WebUI 暴露到回环之外是一条四环的链,断了三环。`PairingToken::mint` 在产品里没有调用方——一个文档写着「把码交回去给人看一次」的类型,从来没让任何人看过。页面拼链接时没带上令牌,而 socket 的 URL 只留了主机名,把查询串丢在了半路。于是一座绑在 `0.0.0.0`、设了 `SPRAWLING_PAIRING_TOKEN` 的城,向每一个对端索要令牌,而它自己的客户端从不发送:**服务端在这个令牌正是为之存在的配置里,拒绝了它自己的 WebUI。**

**控制台答不了问题。** 它对每一个查询的回复,是一段 `sprawling call` 命令的文本——它要求一个已经站在城里的人再开一个终端,从外面来审问它。而那个真能作答的函数,就建在上面一行,只交给了 socket。

**`exec` 的拒绝辞里指向一个谁也装不出来的构建。** 那句拒绝写着「用 program 臂,或者装一个带 `wasm` feature 的构建」,而**那个构建不存在**。它现在存在了,在 `--features sandbox` 后面。在期待它出现在这个归档里之前,请先读最后一节。

## 四个被报成了别的东西的故障

**一个在评审下的会话,把它的决定放上了楼的公共书架。** 处在评审状态的楼会借给每个会话一棵自己的树,好让它写的东西在有人检查之前都不算这栋楼的。而归档的落盘写在了那棵树外面。

**一个打不开的文件,看起来像一个被人写坏的文件。** 读不出来的计划被压成了空字符串,然后被报成「有人改过的计划」;读不出来的交接则等于「没有交接」。现在两者都说出实际发生的事——而这个差别,恰恰在它最不显眼的时候最要紧。

**一次改动可以跑在它自己的记录前面。** 被历史拒掉的一行,现在是一次城从未做出的改动;而合并会等那一行先落。

**一个上限可以丢两次。** 交给另一个居民的活,现在在派它出去的那个上限下跑;回答一次审批也是接着原来的活干,而不是在默认上限下重开。

**同一个请求到两次,是一件活。** 重试和重复帧现在按命令本来就带着的幂等键结算,而不是按内容现造一个。

## 水面之下

派活路径从一个 1069 行的函数变成分散在具名阶段里的 158 行。现在有一道机器闸门会让任何超过 200 行的生产函数构建失败,并且没有为了让它通过而豁免任何一处。一共 1206 条测试、13 道闸门;在这里,一次改动要到它们全绿才算做完。

---

## 怎么跑起来

**下载对应你系统的归档,解压,运行里面的启动器。** 不安装任何东西,也不写这个文件夹以外的任何地方。

| 系统 | 归档 | 运行什么 |
|---|---|---|
| Windows | `sprawling-0.0.2-windows-x86_64.zip` | 双击 `start.cmd` |
| macOS | `sprawling-0.0.2-macos-aarch64.zip` | 终端里 `./start.sh` |

一个控制台窗口会打开并一直开着——**那个窗口就是这座城**。浏览器会打开 <http://127.0.0.1:8787>。在那个窗口里按 `Ctrl-C` 停城。

**这些二进制没有代码签名。** Windows 会说「Windows 已保护你的电脑」:选**更多信息 → 仍要运行**。macOS 第一次会拒绝:从访达右键菜单里打开一次,或者清掉隔离属性。

**在它能做任何事之前,你需要一个能调用的模型**——一个说 OpenAI 或 Anthropic 格式的服务商 API key,或者一次订阅登录。这个程序调度 agent 并记录它们做了什么;它自己不思考。

归档里的 `QUICKSTART.md` 带你走完最初十分钟。每个归档还带一份 `sbom.cdx.json`,是它旁边那个二进制的完整物料清单。

## 开始之前请知道这些

- **从来没有一个真实的会话流经过这个界面。** 我上面描述的每一屏,都是对着测试里构造的记录、在没有浏览器的情况下断言出来的。1206 条测试和 13 道闸门说的是:面对交给它们的数据,那些折叠、那些拒绝、那张变更表是对的。而这个仓库里没有任何一道闸门能做的事是:接上一个服务商,派一件真活,然后**看一眼**。**所以一轮的视图、变更表、文档阅读器,从来没有显示过哪怕一个字节是模型产生的。** 请在评判它们好不好看之前先读这句话——而如果你一上手就发现它不对,那是你能告诉我的最有用的事。键盘和命令面板是例外,那些键是在真浏览器里按过的。

- **这个归档不带执行引擎。** `sandbox` 默认关闭,因为 wasmtime 是一个很大的二进制,这一版我没有重开这笔权衡。所有走沙箱的东西仍然回答「this build carries no execution engine」;在你机器上直接跑程序的那条臂照常工作。想要就自己构建:`cargo build --release -p sprawling --features sandbox`。

- **这一版没有 Linux 归档。** Linux 那条流水线是被裁掉的,不是被调通的。这一版发的是 Windows 和 macOS。

- **首次运行有两步很容易漏**:接上服务商之后,你必须给 `main` **和** `digest` 各选一个模型,否则每一次派活都会被拒,理由是 `no model is chosen for this tag`。

- **从另一台机器够到一座城需要 `SPRAWLING_PAIRING_TOKEN`。** 不设它而绑到非回环地址,是故意被拒的。设了之后,控制台会打印一个带钥匙的 URL——请打开**那个** URL,而不是自己手敲一个,否则服务端会像它当初拒绝自己那样拒绝你。

**其他任何事,发邮件给我。我看得很勤。**
