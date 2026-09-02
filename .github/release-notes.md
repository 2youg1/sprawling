# v0.0.3 — pre-alpha

*中文原文在后半篇。*

**From this release on, every tag carries its date**: `v0.0.3-Pre-alpha-260903`.
A pre-alpha version number says almost nothing about how old the tree is, and
that is the thing you most need to know before downloading one. The two earlier
tags have been renamed the same way, and there is a
[`CHANGELOG.md`](../blob/main/CHANGELOG.md) now, which should have existed from
the first day. What changed in this release, card by card, is in there.

---

This release went into the back end and removed a number of design problems.
Most of what was meant to exist now does, and the gap between this and what I
first sketched is no longer large. If features arrive from here on, it will
mostly be because I found something interesting, not because I was filling a
hole I left.

On *small and good*, small is now holding up. Set it against the other harnesses
that advertise being small — [fx](https://github.com/vercel-labs/fx), say — and
against the ones that advertise session RAM — [jcode](https://github.com/1jehuang/jcode) —
and it does not come off worse, once the WebUI is taken out. The WebUI is the
subject of the next section.

### The measurements behind that

Every reading below is from this release's own build on one machine
(windows-x86_64, i5-1340P), taken with the instrument this repository gates
itself with.

| What | Reading |
|---|---:|
| The binary you download, with the whole WebUI client inside it | **8.88 MiB** |
| The client it carries, gzipped | 657.7 KiB |
| The binary without the client | ≈ 8.25 MiB |
| A city serving — ledger open, socket listening, no model attached | **7.97 MB** working set (2.37 MB private) |
| Process start, warm | 58–63 ms |

From the two projects' own READMEs: fx states a **7.8 MiB** binary; jcode states
**27.8 MB PSS** for one active session with local embedding off, and **167.1 MB**
with it on.

**Be careful what that comparison is.** jcode's number is PSS, a Linux counter,
measured with a session actively running; mine is `WorkingSet64`, a Windows
counter, measured on a city that is serving and idle. Different counters,
different operating systems, different load — and I have not run either project
on the other's platform. Read the table as *this is the weight class*, not as a
benchmark. The one conclusion it does support is that carrying a browser
interface did not cost me the thing I said I would not give up.

---

The front-end problem is not solved, so the issue stays open — I will keep
fixing the WebUI and close it when I think it is worth using daily.

And I have been thinking: a front end is essentially a theme, and it ought to be
replaceable. I am exploring a direction where TUI, GUI and WebUI are a free
choice and open to extension. Everyone has their own taste and their own habits
of work, and models keep getting stronger, so of course people can run a front
end styled however they like — not a colour swap or a background image, but the
interaction logic itself designed around what they prefer. (A well-designed front
end still matters. A carefully designed one beats a good-looking, interesting one
thrown together in an afternoon by a wide margin on productivity and usability.)
Perhaps a UI market later on: install the core, open the market, pick one you
like. Installation gets faster that way, and to some extent it does not even need
to be a market of code — it could be a market of prompts. An agent can easily
learn what the back end offers and what shape the interface is; all the user has
to submit is their own taste (Stardew Valley, say, or a cat house) and a complete
front end is generated from that.

I am pushing hard on splitting files. What we have now is plainly over budget
for a human reader and for an LLM alike, and pressing everything down into a few
hundred lines is the better option.

As for remote control, multi-device collaboration, and agents talking across a
network: I am not trying to make this a universal platform. A single harness
should carry a deliberately limited amount. So I am building
[kusanagi](https://github.com/2youg1/kusanagi) as the answer to that — it is the
more promising project of the two — and I hope it brings some surprises.

For a while there probably will not be any large back-end moves; stability
matters. The next release is again mostly about how to fix the experience and
reduce bugs, and I would like to leave pre-release status as soon as possible.
Along with that, the documentation and the interface text: it is all
agent-written at the moment, some of it long-winded and some of it abstract, and
I will replace it with human-written text wherever I can.

Testing so far is on two browsers, headless Edge and Zen; there will probably
need to be more. The other serious problem is that I have no macOS device, so I
have no way to actually test how it behaves there.

---

## Getting it running

**Download the archive for your system, unpack it, and run the launcher inside.**
Nothing is installed and nothing outside that folder is written to.

| System | Archive | What to run |
|---|---|---|
| Windows | `sprawling-0.0.3-windows-x86_64.zip` | double-click `start.cmd` |
| macOS | `sprawling-0.0.3-macos-aarch64.zip` | `./start.sh` in a terminal |

A console window opens and stays open — **that window is the city**. Your browser
opens at <http://127.0.0.1:8787>. `Ctrl-C` in the window stops the city.

**These binaries are not code-signed.** Windows says *"Windows protected your
PC"*: choose **More info → Run anyway**. macOS refuses the first launch: open it
once from Finder's right-click menu.

**You need a model to call before it can do anything** — an API key for a
provider speaking the OpenAI or Anthropic dialect, or a subscription login. This
program schedules agents and records what they do; it does not think by itself.

---

# v0.0.3 — pre-alpha（中文）

**从这一版起，每个 tag 都带上年月日**：`v0.0.3-Pre-alpha-260903`。一个 pre-alpha 的
版本号几乎说不出这棵树有多旧，而下载之前最该知道的就是这件事。之前两个 tag 已按同样的
规则改名，另外现在有 [`CHANGELOG.md`](../blob/main/CHANGELOG.md) 了——这本该从第一天
就有。这一版逐张卡改了什么，写在那里面。

---

这个版本优化了后端减少了一些设计问题，应该实现的功能也推进得差不多了，同最初设想的
差距已经没有很多，如果未来有功能新增，大抵是发现了新的有意思的功能。当前在小而美上
已经表现不错，对比一下其他宣传体积小的 harness（例如：<https://github.com/vercel-labs/fx>），
Session RAM 的 harness（例如：<https://github.com/1jehuang/jcode>）也不会差了（如果
删去 WebUI 的话，至于 WebUI 的问题下面会讨论）。

### 上面那句话背后的读数

下面每一条都出自这一版自己的构建、同一台机器（windows-x86_64，i5-1340P），用的是这个
仓库给自己设门时用的同一件量具。

| 量的是什么 | 读数 |
|---|---:|
| 你下载的那个二进制，整个 WebUI 客户端就装在里面 | **8.88 MiB** |
| 它带着的那份客户端，gzip 后 | 657.7 KiB |
| 去掉客户端之后的二进制 | ≈ 8.25 MiB |
| 一座正在服务的城——账本已开、socket 在听、没接模型 | **7.97 MB** 工作集（私有 2.37 MB）|
| 进程启动，热态 | 58–63 ms |

取自那两个项目 README 里自己写的数：fx 是 **7.8 MiB** 二进制；jcode 是关掉本地嵌入、
一条活跃会话时 **27.8 MB PSS**，开着则是 **167.1 MB**。

**这个对比要小心它不是什么。** jcode 那个数是 PSS，Linux 的计数器，量的是一条会话真的
在跑；我这个是 `WorkingSet64`，Windows 的计数器，量的是一座在服务但空闲的城。不同计数
器、不同操作系统、不同负载，而且我没有在对方的平台上跑过对方的项目。这张表读作「大致
在这个量级」，不要读成一次基准测试。它唯一支持的结论是：带一个浏览器界面，并没有让我
付掉那件我说过不会放弃的东西。

---

但目前前端问题还没有解决，所以 Issue 依然挂着没关，我会修正 WebUI 问题直到我认为值得
日用再关闭。而且我在想，前端本质上就是主题，应该是可以替换的，正在探索往 TUI／GUI／
WebUI 自由选择和开放拓展的方向发展，每个人都有自己的审美，也有习惯的工作方式，模型
越来越强大，大家当然完全可以使用自己定制风格的前端（当然设计优秀的前端仍有意义，毕竟
经过精细设计的前端在生产力和可用性上会比随手做出来好看有意思的强上很多），不是换色／
背景图，而是完全基于自己的喜好设计交互逻辑。也许未来可以设计一个 UI 市场，下载本体后
直接打开市场选一个喜欢的安装。这样的好处是安装会变得更快，而且一定程度上来说甚至都
不需要做一个基于代码的市场，可以是一个基于 Prompt 的市场，Agent 可以轻松的了解后端有
哪些功能，接口的形状什么样，只需要用户提交一个自己的审美偏好（例如用星露谷小镇或者
猫窝作为主题）就可以生成一整个完整的前端。

我在努力推进拆分文件的事情，目前很明显对于人还是 LLM 都完全超额了，代码全部压入几百行
是更优选。

至于远控与多设备协作，远程多 Agent 交流方面，我没有想着把它做成一个万能平台，单一
Harness 应该承载的内容必须精简有限，所以我正在开发 <https://github.com/2youg1/kusanagi>
作为一个解决方案（这是个更有潜力的项目），希望它可以带来一些新的惊喜。

接下来一时半会大概不会有后端大动作（稳定很重要），下个版本依然是思考怎么解决体验问题
和减少 BUG 为主，希望尽快脱离 pre-release 状态。以及各种文档和 UI 文字的优化，目前都是
Agent 创作，有些表述拖沓和抽象问题，会尽可能换成人工创作版本。

目前的测试都基于无头 edge 和 Zen 两个浏览器，未来可能要更多测试，另外一边严重的问题是
我没有 MacOS 设备我没办法实际测试其表现。

---

## 怎么跑起来

**下载对应系统的归档，解压，运行里面的启动器。** 不安装任何东西，也不往那个文件夹之外
写任何东西。

| 系统 | 归档 | 运行什么 |
|---|---|---|
| Windows | `sprawling-0.0.3-windows-x86_64.zip` | 双击 `start.cmd` |
| macOS | `sprawling-0.0.3-macos-aarch64.zip` | 终端里 `./start.sh` |

会打开一个控制台窗口并一直开着——**那个窗口就是城**。浏览器会打开
<http://127.0.0.1:8787>。在那个窗口里 `Ctrl-C` 停城。

**这些二进制没有代码签名。** Windows 会说「Windows 已保护你的电脑」：选**更多信息 →
仍要运行**。macOS 第一次会拒绝：在访达里右键打开一次即可。

**它能干活之前你需要一个可调用的模型**——一个说 OpenAI 或 Anthropic 线格式的服务商
API key，或者一个订阅登录。这个程序调度 Agent 并记录它们做了什么；它自己不思考。
