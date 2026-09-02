# sprawling

**在你自己的机器上，把一群 Agent 组织成一座城。一个 Rust 二进制，界面在浏览器里。**

![binary](docs/badges/release_binary.svg) ![client](docs/badges/frontend_artifact.svg)

徽章称的那个二进制就挂在[最新 release](../../releases/latest) 上；两个数都由构建门的同一次称重渲染，没有人往文档里手写体积。

> **状态：pre-alpha，研究与开发中。** 主回路是通的：在浏览器里注册一个 provider、盖一栋楼、派一件活，模型真的会调用工具并把文件写进那栋楼；多个 Agent 可以同时开工，各在各的房间。
>
> 还没通的写在[现在能做什么](#现在能做什么还不能做什么)那一节。把真活交给它之前，请先读那一节。
>
> English: [README.md](README.md)

**优点**：体积小；概念超级潮酷；面向多 Agent，而不是一个 Agent 挂一圈扩展。

**缺点数不胜数**：不由中转站资助、也不由实验室维护的学生项目；没有二次元形象；WebUI 想做好，能力实在差点；功能的稳定性与可用性都还要调。

---

## 为什么做它

我不想24/7守在电脑面前，直到5h额度撞墙再去睡觉，你也不想。

我换过很多Harness，有些理念落后，有些超出实际：就以RSI来说，在LLM本身脱离无状态之前，Harness能做的只是不断地针对最新的模型做适配和学习公司的现有业务流程并更高速地运行，前者的趋势是消融实验，后者则需要隐私。

越来越多的小规模公司正在出现，它们是有着大量Agent开发在线服务的小型团队，其中99%就是Markdown集+几个天才。

因而我想要制作一个跟上新生的多Agent（Graph engineering）又同时能务实地处理RSI和记忆相关概念风潮的Harness，我将务实的可拓展、节省用户精力、实验性的agent规模化的成本控制、隐私与可靠性、长时运行能力这些放在了设计的核心，并结合了一些城市学与社会学的内容设计了sprawling。

Agent的能力和规模越强，人的注意力就越贵，我不想sprawling成为无数希望劫持你注意力应用中的一个。sprawling区别于常规Hanress聚焦于编写Prompt，最佳使用实践应是转向Loop，安排工作流，让Agent开发sprawling，接管你的固定工作……让你本人专注于新业务的设计，新技能的学习，偶尔回来看一眼跑的怎么样。

诚实地说目前还没有一种多Agent方案提升的性能对得起规模化提升的成本，但探索这项技术在自动化业务，社会模拟以及AI对齐方面的研究刚刚起步，我们还需要花费很多精力和资源探索Agent集群场景下模型的交互行为、协作效率与社会性表现。

Agent记忆的确是实现RSI很重要的途径，但不是依靠Harness做注入，你的文件、代码，文档库就是记忆，Agent真正和你一起成长的尝试在LLM脱离无状态之前大多是对模型的拖累。

如果你想要用自己喜欢的Harness可以试一下RefRain。sprawling主要面向为小团队持久化运营和学术（无论计算机还是人文社科）研究平台，目前还处于研究与开发阶段，欢迎一起开发，也欢迎和我联系/讨论。

除了迁移必要的业务skill/MCP/ACP之外，推荐暂时保持精简，在使用中遇到问题时再手动追加内容，即使是相同的模型搭配不同的Harness都会有完全不同的行为。

我用的电脑不好，所以我不会放任多Agent产生性能开销指数增长的问题，也适合部署在你的旧电脑或云电脑上。

我不卖API也买不起装你信息的硬盘，因而数据都留在本地，我设计了专门的保密楼，配上本地模型完全可以用于处理隐私数据，但这也意味着我没法运行巨大规模的测试。

---

## 它是什么

一个二进制，一页浏览器界面。客户端是 Rust 编译成的 WebAssembly，嵌在这个二进制里，所以构建本仓库不需要 npm 也不需要 node。这是本仓库发出的那个客户端的性质，不是对你的规定：**客户端可以换。** 凡能说 `crates/channels` 那套 WebSocket 协议的都是客户端，用你与你的 Agent 写得最好的语言写即可。本仓库曾经有一道禁 JavaScript 的门，它被删掉正是因为这个理由——它排除的是架构，而不是缺陷。

磁盘上的目录树就是空间：一座 **City** 是一棵目录树，一个项目是一栋 **Building**，一个 Agent 的工位是一个 **Room**。

**一个地址同时定死四件事。** `lab/room1` 说的是文件在哪，也是这个 Agent 能写哪些文件、它带着什么上下文开工、它向谁汇报。这四件不需要任何机制维持一致，因为它们本来就是同一个事实。

**Agent 自己找到彼此并说上话，不需要你在中间传话。** 一跑可以问本楼还有谁，拿回每一个它够得着的地址，以及那位住户自己的 `URBANITE.md` 写的「什么样的活该拿给我」——于是「该找谁」有了一个不靠猜的答案。对方正在干活，话从门缝塞进去，落在他下一次工具结果的末尾；对方没在干活，城市为他开一跑。两条路上这句话都带着 `@` 与发件人的地址——那也正是回信要填的地址，而**一位居民永远冒充不了你**：这是类型的性质，不是一条约定。

**Ledger 是唯一历史。** 任何效果先成为一条事件，再成为效果。界面上的每一个视图都是这条事件流的 projection：删掉一个，从 Ledger 重建，字节一致。日志被改动一个字节，验链会报出行号并拒绝往下走。

**删除自带回退路径。** 表示「丢弃一个文件」的类型没有不带 Restoration 的构造函数——「删了回不来」不是运行时被拒绝，是根本写不出来。回收站里每一行都带着能把它取回来的那句话。

**成本按五个维度归因**，每个维度都精确加总到 provider 实际计费的那个数。provider 没给价格时（比如订阅），界面直接说没有价格，而不是打印 `$0.00`：零和未知是两件不同的事。

**界面的设计目标是别烦你。** 没有小红点，没有未读数，没有无限流，进度条不做动画。只有一件事会打断你：需要人来决定的事。其余的都在你会找到的地方等着。

**每个组件都把自己的 SPEC 放在旁边。** `crates/<crate>/<crate>-SPEC.md` 写着那个 crate 的接口与它们背后的理由，先于代码存在、也先于代码改动。于是人和 Agent 改这个项目时读的是同一份文件，而公开面与 SPEC 一旦走散，一道门就会拒绝这次改动。

**有些状态不是被校验，而是不可表示。** 伪造一个事件引用、反序列化出一个「已完成」、隔着网络录入凭证、画一个没有分母的百分比——这些在类型系统里写不出来。每一条在测试里都有一个编译失败反例，因为「写不出来」本身就是一个需要被证明的断言。

## 跑起来

### 快速上手

1. 从 [latest release](../../releases/latest) 下载对应系统的压缩包。
2. 解压到任意位置。
3. 运行 **`sprawling.exe`**（Windows 直接双击）或 **`./sprawling`**（macOS）。本次发布不构建 Linux 包。它在建任何东西之前先问你一句。

这就是全部安装。不写注册表，不装服务，那个文件夹之外一字不动，删掉文件夹即全部清除。会弹出一个控制台窗口并一直开着：**那个窗口就是那座城**。浏览器会自己打开 `http://127.0.0.1:8787`；没打开就自己输这个地址。在那个窗口按 `Ctrl-C` 停城。

二进制没有代码签名，所以第一次运行会被拦：Windows 提示「已保护你的电脑」，选**更多信息 → 仍要运行**；macOS 首次拒绝，在访达里右键打开一次即可。

**它自己不会思考，开工前你得先给它一个模型**：一把说 OpenAI 或 Anthropic 兼容格式的 provider 的 API key，或者一个订阅登录。sprawling 负责调度 Agent、记录它们做了什么并展示给你。

### 从终端

拿到那一个二进制就够了。不装 npm，不装 node，不装任何运行时。启动器跑的就是下面这一条：

```bash
sprawling up [city-dir] [addr]      # 城不在就先建，然后起服，然后开 WebUI
```

拆开来写：

```bash
sprawling init  <city-dir>          # 建一座城；城名写进创世记录
sprawling serve <city-dir> [addr]   # 起控制面；默认只听回环
# 然后打开 http://127.0.0.1:8787
```

> **不要用 `cargo install` 装它。**客户端是先编好再嵌进二进制的 WebAssembly；单跑 cargo build 跑不了那一步，装出来的二进制页面是空白的。要么拿 release 压缩包，要么用 `just dist` 自己构。

页面上四步，大约十秒：

1. **settings**——填 provider 的 base URL、dialect（OpenAI 或 Anthropic）和 key。key 直接进操作系统的凭证服务，页面此后只看得到 `secret:realm/name` 这样一个引用。
2. 同一页，按标签选模型：`main` 负责思考，`digest` 替它读长文档。
3. **city**——盖一栋楼。
4. 底部的 control surface——地址、要产出什么、什么算完成。**它不问预算**：一件活跑之前没人说得出它值多少钱，订阅更是没有单价；花了多少事后从记录里报出来。**对话也不限额**——居民互相叫醒之后要谈多久，是他们自己的事。约束单一一跑的是它的回合上限。

其余命令：

```bash
sprawling resume <city-dir>         # 重启之后：验链、关掉结果已丢失的工具调用、报出谁在等人
sprawling fork <city> <run> <seq>   # 从某个 Run 的某一步分叉出一条谱系
sprawling adopt <city> <dir>        # 把一个已有目录收编成楼，不覆盖任何文件
sprawling replay <ledger-dir>       # 离线验链，只读
sprawling export <city-dir> <file>  # 打包一座城；清单就是完整性判据
sprawling restore <file> <city-dir> # 在另一台机器上解开
sprawling status [--deps]           # 这台机器的情况；--deps 列出编进来的依赖
sprawling help                      # 所有命令，一屏列完
```

一个命令也不带地启动它——比如双击——它只给一屏：写出它打算建在哪里，等你点头才动手。建城要写创世记录，而那件事不会因为有人双击了一个文件就发生。

从空目录到第一个 Run，一步不跳的走法在 [`docs/getting-started.zh-CN.md`](docs/getting-started.zh-CN.md)。

## 五个词

| 词 | 是什么 |
|---|---|
| **City** | 一台机器上的一座城：一棵目录树、一本 Ledger、一部完整历史。两座城之间恒不互相引用。 |
| **Building** | 城里的一栋楼，一栋楼一条业务线。配置、Archive、WriteDomain 都以它为范围。 |
| **Room** | 楼里的一个房间，也就是一个子目录。一个 Agent 在一个房间里干活。 |
| **Run** | 一次有始有终的工作。**Resident 是身份，Run 才是成本**——这两个数字差两个数量级。 |
| **Ledger** | 唯一历史。一行一件事，只追加，可离线验链。 |

其余词汇在 [`docs/glossary.md`](docs/glossary.md)。

## 现在能做什么，还不能做什么

**能做**，每一条背后都有端到端断言或真实测量：注册 provider 并选模型；盖楼、派活；模型真实调用 tool 并把文件写进那栋楼；**居民自己找到彼此、说上话、互相叫醒，全程没有人传过一句话**——两位居民对着真实 provider 把一笔价格谈到了白纸黑字；给一栋楼配外部 MCP server；多个 Agent 同时开工，各有自己的 git worktree，改动要别人验过才能并回楼里（这是编译错误，不是一条规矩）；十个页面（城市、直播、审批、回收站、归档、成本、账本、楼、房间信箱、设置）；停城与放行；离线验链；导出一座城并在另一台机器上恢复。

**没做，以及为什么**：

| 没做的事 | 理由 |
|---|---|
| OS 级 sandbox | 要逐平台实现，本机只验得了三分之一。没验证过的隔离比没有隔离更坏，因为它会被当成防线。所以今天的说法是「一次删除可以被撤回」，不是「一次删除不会发生」 |
| CI 里的浏览器端到端 | 回路是本机的一条命令，不是一道门 |
| 可复现构建 | 夹具写好了，让两次构建字节一致的编译开关还没设 |
| 把花费摊到 skill 上 | 这是决定不是欠账：一次 tool 调用不发生在某个 skill「之下」——skill 是 prefix 里的一行披露，不是调用上下文，按调用摊钱等于发明一个基准 |

## 你可以换掉哪些零件

我不卖 API 也不代管账号，所以外面的东西全都接在 seam 上，换掉不需要改别处：

| 零件 | 住在哪 | 怎么换 |
|---|---|---|
| 订阅登录情报（跟随 openai/codex 与 earendil-works/pi） | `gateway::oauth_profiles`（只有数据零分支）、`gateway::credential`（流程与续期） | 加一行 profile。**凭证保管恒不外包**：明文只到本机凭证服务 |
| 模型 endpoint 与 dialect | `gateway::endpoint`、`gateway::dialect`；本地推理走 `gateway::native` | 设置页里填 base URL 与 dialect；本地模型直连，不过网关 |
| SaaS 与外部工具（[Composio](https://composio.dev) 是其中一个 MCP server） | `protocol::mcp` 的 `Outbound` seam、`bin::mcp_stdio` 与 `bin::mcp_http`、楼的 `CONFIG.toml` | 改一个 URL 或一条命令就换了 server；保密楼一个都不起 |
| sandbox | `runtime::sandbox` seam（今天的适配器是 wasmtime fuel） | 实现这道 seam，过它的 conformance 断言套件 |
| 客户端 | `channels::wire` 是唯一 API 面 | 想写第二个客户端，就对着这套线格式写 |

每一件的位置与替换步骤在 [`ARCHITECTURE.md`](ARCHITECTURE.md)。

## 姊妹仓库：[kusanagi](https://github.com/2youg1/kusanagi)

城内的历史是**一条链**。任何效果先成为一条事件落在唯一一本只追写的 Ledger 上，再成为效果；那一份全局全序是五个平常问题能被回答的前提：这件活谁认领的、谁的改动撞上了谁、哪个目标赢、这句话是不是已经送过、我读之后还有没有人写过。五个问的都是*谁先*，而一座把历史拆开的城就再也说不出来。

机器与机器之间，同一份全序恰恰是不能有的东西。`kusanagi` 是给 Agent 的去中心化协作网络：**一对一条链**，每一个地址都推得让同一场对话的任两条投递在承载它们的主机看来无法关联，而那台主机双方都不运营、也都不信任。这里的全局全序是一件旁观者读得到的事实。

城内单链，城际分链。两个仓库是同一个回答的两半——Agent 怎么保住一份信得过的历史；单看任一个，都像缺了另一半。

## 它听在哪里，凭证在哪里

**默认只听回环。** 要让同一网络里的另一台机器连进来，绑一个非回环地址并设 `SPRAWLING_PAIRING_TOKEN`；没有配令牌就**拒绝启动**，而不是先起来再逐个拒连接。再往外，这个仓库不带隧道也不带中继：那两样各有自己的信任模型，替你选一个就是替你做安全决定。

**凭证明文不进任何文件、任何事件、任何日志。** key 进操作系统的凭证服务，配置里只留 `secret:realm/name`。模型输出在入账前过同一道 secret 扫描，所以一个被模型复述出来的 key 不会变成永久记录。

## 文档

除本页与 getting-started 外，文档是英文的。

- 刚到，想知道这是什么：读完本页即可；再深一层看 [`docs/glossary.md`](docs/glossary.md)。
- 要用它干活：[`docs/getting-started.zh-CN.md`](docs/getting-started.zh-CN.md) → [`docs/operating.md`](docs/operating.md)。
- 要改它：[`ARCHITECTURE.md`](ARCHITECTURE.md) → [`AGENTS.md`](AGENTS.md) → 相邻模块的代码与测试。

另有 [`CHANGELOG.md`](CHANGELOG.md)（每一版改了什么，以及它声称的那些数字出自哪台机器）、[`docs/logging.md`](docs/logging.md)（日志为什么不是历史）、[`docs/third-party.md`](docs/third-party.md)（站在谁的肩上、许可义务）。[`docs/City.md`](docs/City.md) 与 [`docs/templates/`](docs/templates/) 是城写进楼里的那几份文档——Agent 读它们，你也可以读。

## 参与

先读 [`AGENTS.md`](AGENTS.md)，三十秒版：

```bash
cargo install just cargo-nextest --locked
just check
```

这一条绿了，一次改动才算完成。**PR 正文、issue 与评审意见可以用你的母语写**；如果愿意，附一份对照译文（母语非英文就附英文，英文就附中文）——有对照，人和 Agent 都读得更快，也不会把意思翻错。

## 站在谁的肩上

登录一个 provider 要知道一小把端点和参数。与其自己盯着那些 API 文档，我跟随两个仍在维护的项目：

| 项目 | 许可 | 跟随什么 |
|---|---|---|
| [openai/codex](https://github.com/openai/codex) | Apache-2.0 | OpenAI 的订阅登录：端点、client id、scope、device-code 流程 |
| [earendil-works/pi](https://github.com/earendil-works/pi) | MIT | Anthropic 与其余订阅 provider 的同类情报 |

**跟随的是情报，不是代码。** 端点和参数是事实；流程与凭证保管在这里自己实现。

外部应用的连接同样外包出去：城对任意 MCP server 说 MCP，Composio 是其中之一。这个仓库不带任何人的 key、不替谁付钱、不做代理。完整清单、怎么复核、许可怎么处理在 [`docs/third-party.md`](docs/third-party.md)。代码依赖的许可由 `cargo deny` 逐个核对，白名单是 [`deny.toml`](deny.toml)。

## 许可

MPL-2.0，见 [`LICENSE`](LICENSE)。
