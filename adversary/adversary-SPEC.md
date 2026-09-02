# adversary-SPEC

> `adversary/` —— 仓外的对抗性性质检验器。它不是 crate，不进 workspace，不进发布物，不进 `just check`。
>
> 权威顺序：用户裁决 → `ARCHITECTURE.md` §8「the wire is the whole API; a second client writes against it」→ 本文 → 代码与测试。本文先于代码改动。

## 1 需求拆解

`ARCHITECTURE.md` §8 把线格式定为整个 API，并明说「用任何语言写第二个客户端都是支持的」。本目录行使这一条：它是**第三个**客户端，写在仓外，用来攻击而不是使用。拆成六个可独立验收的最小单元：

| 单元 | 交付物 | 独立验收 |
|---|---|---|
| U1 门 | `Door`：以子进程驱动已构建的二进制，把 stdout 的每一行解成 `Frame` | 一条 init→serve→create_building→city_view 的轨迹被解析成结构化值；未知的帧类当场报错而不是被忽略 |
| U2 场地 | `Ground`：一次性城目录、一个被端起来的进程、一个端口，以及磁盘上的敌意动作 | 场地退出后不留文件也不留进程；`stored` 看到的正是账本目录里的字节 |
| U3 模型 | `StateModel` / `RunModel`：动作集合与后置条件 | 随机轨迹全通过；把 `Halt` 变成 no-op 则立即判红 |
| U4 定向对抗 | dynamic logic：任意前缀 ＋ 一次 `Halt` ＋ 任意后缀 | 该性质在随机轨迹上成立，且停摆后每一次派活都被拒 |
| U5 回归 | 反例最小化后渲染成 Rust `#[test]` | 渲染结果与仓内那个 Rust 文件逐字节相同，而该文件由 `cargo test` 编译运行 |
| U6 历史 | 任意轨迹之后，账本离线自证；改一个字节则不能自证 | `replay` 在干净轨迹上恒绿、在翻过一位的轨迹上恒红 |

**不负责**：任何规则的再实现（链哈希、`IdemKey` 派生、写域判定、份额守恒）；任何 Rust 侧的构建闸门；任何随产品交付的东西。三者中任何一条被违反，本目录应当被删除而不是被修补。

**U6 是本目录相对 kusanagi 的增量**，理由在产品而不在方法：kusanagi 的宿主只持有互不关联的 drop，而一座城把**一条全序的历史**写在磁盘上，于是「任意轨迹之后历史仍然自洽」是一条可以对着随机轨迹反复问的性质，而不只是一次定点检查。

## 2 验收标准

1. `cabal test` 全绿。
2. 没有 GHC 的机器上 `just check` 的行为与本目录不存在时**逐字节相同**；`just adversary` 打印 `skipped: GHC is not installed` 并返回 0。
3. 模型不预测任何哈希、`seq`、时间戳或 `IdemKey`。凡断言只谈**两条轨迹之间的关系**，或**门对调用方的承诺**（稳定错误码）。
4. U5 渲染出的 Rust 源码与 `crates/sprawling/tests/from_adversary.rs` 逐字节相同。
5. **咬得动的证据**：`Model.hs` 的 `haltIsHonoured` 在把 `Stop` 从模型里摘掉后必须失败。一个永远为真的性质与没有性质等价。

   **已演示。** 把 `refusal` 里 `Work` 那条 `not (standing world addr) -> Just (Code "E_GATE_DENIED")` 删掉后，`halting` 判红并给出 `refused with Code "E_GATE_DENIED" where Just (Code "E_CONFIG_INVALID") was owed`；恢复后 6.08 s 转绿。它咬得动的是**守序**，而不只是「停摆时派活会失败」。

## 3 假设与歧义

| 歧义 | 假设 | 何时失效 |
|---|---|---|
| 门的形状 | `sprawling call <frame> --at <addr> --quiet-ms <n>`：stdout 每行一枚 JSON 帧，stderr 一行计数，退出码 0／1／2 | 线格式换传输时门变成它的 schema，改 `Door.hs` 一处 |
| 城是什么 | 一个本地目录，`init` 造它，`serve` 端起来，账本在 `.sprawling/ledger/` 下按段分文件 | 布局改变时 `Ground.hs` 的敌意动作判红，属预期 |
| 静默 | 门的第三种回答。**不是接受**——见 §10「静默不是接受」 | 若将来 `call` 改为「命令被受理才返回」，`Quiet` 这一支变成异常而不是取值 |
| provider | 一个都不挂。于是每一次 `Dispatch` 在配置这道门上被拒，而模型知道这一点 | 挂上任何真 endpoint 后本目录会把 `E_CONFIG_INVALID` 报成失配，届时模型要学会第二种世界 |
| 时钟 | 只用于超时，从不被预测 | —— |
| 端口 | 从 47100 起向上探，第一个能答 `city_view` 的即用 | 机器上有别的东西占着整段时判红并说明 |

## 4 现状分析

Rust 侧的验收测试全部是**具体轨迹**：`crates/sprawling/tests/assembly_door.rs` 证明「这一条路走得通」，不证明「任何一条路都走不出去」。本目录的全部增量在后者，以及三件 Rust 侧写不出的事：

1. **门的承诺只在门外才可观测。** 退出码、两个流的分工、静默与拒绝的区别，都是「一个 agent 拿这个二进制写脚本」时遇到的事实，而进程内的测试永远看不见它们。
2. **敌意的磁盘不是被 mock 的磁盘。** `memory::fault_fs` 是一个确定性掉电模型，它回答「我们设想的坏」；本目录直接翻掉账本里的一位，回答「随便一位坏了会怎样」。
3. **任意前缀与任意后缀。** 只会均匀随机生成的东西是 fuzzer；把攻击命名出来再对它前后做全称量化，才是对手。

### 第一个发现（首次探门，尚未由随机轨迹重现）

**`sprawling call` 在拒绝没赶上静默窗口时退出 0。** 实测：`AttachEndpoint` 指向一个连不上的 base URL，产品侧 15 s 探测超时，而客户端默认静默窗口 2 s。

```
$ sprawling call '{"command":{"attach_endpoint":{...,"base_url":"http://127.0.0.1:9/v1",...}}}'
1 frame(s), 0 refusal(s)          # stderr
$ echo $?
0
```

同一次操作，城自己在诊断日志里写下：

```
AttachEndpoint refused: E_PROVIDER: cannot list models on ... (os error 10061)
the refusal above reached nobody: the peer that asked had closed its socket
```

把窗口放到 20 s，同一条命令 22.1 s 后返回退出码 1 与那条拒绝。

**诊断**：城是诚实的——它知道拒绝没送到，并且说了出来。缺陷在门：`main.rs` 的 rustdoc 写着 *"Exits 1 when the city refused something, so an agent driving this learns the outcome from the exit code rather than by parsing JSON"*，而实际语义是「**在静默窗口内没有拒绝到达**」。对一个拿退出码做分支的 agent，这两者的差别是把一次失败读成一次成功。

**处置**：本目录的 `Door` 因此把静默解成 `Quiet` 而不是 `Accepted`（§8），并用 `exitCodeMeansWhatItSays` 这条性质把它钉住。是否修产品、怎么修（受理即答／退出码分第三档／窗口随动词而定）是 Rust 侧的一次裁定，不由本目录决定。

### 第二个发现：一次被拒的派活写进了城里

随机轨迹在第 2 个样本上失败，收缩 4 次后得到两步：`Raise "acme"`／`Work "gamma"`（一个从没立过的地址）／`Look`——`Look` 看见 `["acme","gamma"]`，而只有 `acme` 被立过。追下去是同一条缝的两个症状：

```
派活到一个没立过的楼 → E_CONFIG_INVALID「no model is chosen for this tag」
磁盘上留下           城根/acme/one/JOB.md、城根/gamma/one/JOB.md
账本里               只有 seq 0 city_initialized，一条都没有
```

**诊断**：`assembly/dispatching.rs:179` 的 `dispatch_in` 顺序是「判停摆 → 写 JOB.md → 落 CAS → 解析模型 tag」。它开头那句注释把该守的规矩写得一字不差——*"Nothing is written before the city agrees to take the work: a halted city that laid a job file down would leave a task in a room no run ever opened."*——**停摆那道门守住了它，配置那道门在写之后才判**。楼是否存在则从头到尾没有被问过。

于是：任何调用方派活到任意地址，都会在城根下造出目录树，而这次派活以失败告终、账本一字未记。这与 `ARCHITECTURE.md` §5 第 4 条（*every effect becomes an event first*）直接冲突——磁盘上有人看得见、而城的历史无法交代的文件。

**边界已量过，不夸大**：保留子树是守住的。`Work ".sprawling/evil"` 得到 `E_INVALID_ARGS` 且一个字节都没落地，所以这不是写域逃逸，而是「判据晚于副作用」。

**处置**：修法归 Rust 侧——把能拒绝的判断全部提到第一次写之前。本目录不猜该怎么改，只把两条断言留在那里等它变绿：`nothingBehind`（磁盘）与 `listsOnlyRaised`（`city_view`），**因为一次红应当说出是哪个缺陷，而不是说出它碰了几个测试**。

**已修（card V3.51「a dispatch the city will not take leaves no room behind」）。** 直接量过：向没立过的 `gamma` 派活，城答 `E_CONFIG_INVALID`，城根下仍然只有 `City.md`。两条断言随之转绿，改入 `a refusal costs nothing` 一组留着——它们此后守的是那次修复确立的**判据先于副作用**这个次序，而不是当时那一行代码。

### 第三个发现：一把每条命令都必须带、而没有人读的钥匙

线格式要求 23 个状态变更命令**每一个**都带 `IdemKey`，`kernel::gate::dedup` 把这道门实现成一个纯函数，而 `rg` 在它自身模块与自身测试之外**找不到任何调用者**。可观测的后果，全程只经门：

```
halt city --idem K   → city_halted
halt city --idem K   → city_halted      # 同一条命令，同一把钥匙
replay               → 账本里两条 city_halted
```

同样地，两条地址不同的 `create_building` 共用一把钥匙，两栋楼都立了起来。

**诊断**：`IdemKey` 存在的理由是让**重试无害**——客户端发出命令、连接断了、再发一次，不应当因此做了两次。今天这条保证没有承兑人。`gate::dedup` 自身没有错，错在它没有被接到 `assembly` 的命令路径上。

**处置**：归 Rust 侧裁定，两条路都成立——把 `gate::dedup` 接上去，或者停止在线格式上强制要求这个字段。一把必须带而无人读的钥匙，是门做出而不兑现的承诺。本目录只留 `keyUsedTwice` 一条断言等它变绿，方法是问账本的离线校验器「链走到哪了」，两次读数必须相等：**不预测任何 seq，只谈两次观察之间的关系**。

`Look` 因此不进随机生成器（`Model.hs` 记了理由）：一条随机轨迹里只要出现一次被拒的派活，其后每一个 `Look` 都会为同一个原因失败，那会把一个缺陷报成许多个，并把下一个缺陷藏在它后面。

## 5 权威信源

| 事实 | 来源 |
|---|---|
| `quickcheck-dynamic` 4.0.1，`StateModel` 与 `RunModel` 分属两个类型类，带 dynamic logic | 上游 README、Hackage |
| 本机 GHC 9.10.3 / cabal 3.16.1.0 | `ghc --version`、`cabal --version` 实测 |
| `WIRE_V = 13`，23 个 Command、14 个 Query | `crates/channels/src/wire.rs`、`command.rs` |
| 35 个稳定错误码 | `crates/kernel/src/error.rs` 的 `AxCode::ALL` |
| `IdemKey` 形如 `idem1-` 加 32 位小写十六进制 | 门的拒绝原文实测 |
| 模板只有 `minimal` 与 `confidential` | 门的拒绝原文实测 |

**与 kusanagi 的一处方法差异**：kusanagi 的门是一次性 CLI，`Ground` 只造目录；sprawling 的门要一座**被端起来的城**，于是 `Ground` 持有一个进程的生死。这不是把 `Ground` 变重了，而是把「场地」这个词在本产品里的真实所指写清楚：一座城不是一个目录，是一个目录加一个在服务它的进程。

## 6 命名统一

`Address`、`Building`、`Run`、`Ledger`、`Seq`、`Refusal` 一律沿用 `docs/glossary.md` 与 `ARCHITECTURE.md` 的词表，Haskell 侧不得另起名字。本目录只新增三个词，各自只指一件事：

| 词 | 它是什么 |
|---|---|
| **Door** | 已构建二进制的 `call` 门面，唯一知道有个可执行文件存在的地方 |
| **Ground** | 一次性的场地：一座被端起来的城、它的端口、它的账本目录，以及磁盘可以施加的敌意 |
| **Trace** | 一串动作及其观察结果，是本目录唯一的断言对象 |

## 7 模块边界

```
adversary.cabal              工程定义；不被任何 Rust 构建读到
src/Sprawling/Frame.hs       线格式的代数镜像。只解析，不判断
src/Sprawling/Door.hs        唯一知道二进制存在的地方
src/Sprawling/Ground.hs      一次性场地，以及磁盘的敌意
src/Sprawling/Model.hs       状态模型、后置条件、定向对抗场景
src/Sprawling/Regression.hs  反例 → Rust #[test]
test/Main.hs                 tasty 入口
```

依赖单向：`Model` → `Door` → `Frame`，`Model` → `Ground` → `Door`，`Regression` 只依赖动作类型。`Frame` 不 import 任何本工程模块。

`Ground` 依赖 `Door` 而不是自己起进程：**「二进制在哪」只允许有一个答案**，而场地要用它做三件事（`init`、`serve`、探活）。

## 8 接口先行

```haskell
-- Frame.hs —— 线上说了什么
data Frame = Welcomed Welcome | Happened Record | Answered Text Value
           | Refused Complaint | Streamed Text Text
data Complaint = Complaint { code :: Code, action :: Text, subject :: Text
                           , recovery :: Text, retriable :: Bool }
data Record = Record { recSeq :: Word64, recPrev :: Text, recKind :: Text
                     , recWho :: Text, recRun :: Text, recData :: Value }

-- Door.hs —— 怎么问
newtype Door = Door FilePath
data Said = Said { saidFrames :: [Frame], saidExit :: Int }
data Answer = Accepted [Frame] | Denied Complaint | Quiet   -- 见 §10
discover :: IO (Maybe Door)
raise    :: Door -> FilePath -> IO ()                 -- init
serve    :: Door -> FilePath -> Port -> IO ProcessHandle
ask      :: Door -> Port -> Verb -> IO Answer
verify   :: Door -> FilePath -> IO (Either Text Word64)  -- replay

-- Ground.hs —— 在哪里问，以及磁盘怎么撒谎
withGround :: Door -> (Ground -> IO a) -> IO a
portOf     :: Ground -> Port
ledgerOf   :: Ground -> FilePath
stored     :: Ground -> IO [(FilePath, ByteString)]
corrupt    :: Ground -> IO ()                          -- 翻一位：最老那条记录的中点
tear       :: Ground -> IO ()                          -- 截尾：最新那段的末 20 字节
duplicate  :: Ground -> IO ()                          -- 重放：把最老那条记录再写一遍

-- 三个敌意动作各自回答一件事，不得合并：`corrupt` 问检测，`tear` 问恢复
-- （断尾修复是产品**故意**支持的路径），`duplicate` 回到检测。

-- Model.hs —— 断言什么
instance StateModel World
instance RunModel World (ReaderT Kit IO)
haltIsHonoured :: DL World ()

-- Regression.hs —— 交付什么
sequenced :: [Any (Action World)] -> Actions World
coherent  :: Actions World -> Bool
render    :: Text -> Actions World -> Text
```

`ask` 返回 `Answer` 而不是抛异常：被拒绝是产品的正常输出，而**解析失败**才是异常——门的形状变了，测试应当当场停下，而不是把新形状当成一次拒绝。

## 9 工作流程

1. `just adversary` 先 `cargo build -p sprawling`，把二进制路径经 `SPRAWLING_BIN` 传给 cabal。**唯一一处知道二进制在哪的地方是 justfile**。
2. `cabal test` 跑 tasty：先跑 U5 的渲染对拍（毫秒级，先失败先止损），再跑 U1 的门契约，再跑 U3 的随机轨迹，然后 U4 的定向场景，最后 U6 的磁盘敌意。
3. 反例出现时，`Regression.render` 把最小化后的轨迹写成 Rust 源码打到 stderr，并给出它该被放在哪个路径。
4. 人把那个文件提交到 `crates/sprawling/tests/`。**知识就此迁移到 Rust，Haskell 不保留它。**

## 10 实现逻辑

**门**：`readProcessWithExitCode` 之外自起进程，因为要拿到 stdout 的**逐行**而不是一整块，且要能杀掉 `serve`。退出码 0／1 都要读 stdout；退出码 2 是用法错，抛出。

**静默不是接受。** `ask` 在拿到 `welcome` 之后若一个帧都没再来，返回 `Quiet`。模型对每个动作声明它期待哪一种回答，**`Quiet` 从不满足任何期待**——它只在 `exitCodeMeansWhatItSays` 那条性质里作为被观测对象出现。

静默窗口取 250 ms，而不是第一版那个大于 `PROBE_TIMEOUT_MS` 的 30 s。`sprawling call` **要等满静默窗口才返回**，于是窗口是每一个动作都要付满的价钱：30 s 那版跑了 782 秒也没跑完一次。短窗口在这里是诚实的，理由在模型而不在计时：它一个 endpoint 都不挂，每条命令都在本地磁盘与回环上答完，实测均在 1 ms 以内。慢路径自身的危险由「模型里没有一个动作走它」来保证，而不是靠等。

**模型**：`World` 只记一个用户记得住的东西——哪些楼立着、哪些范围停摆着、哪些楼挂着 pursuit、以及有没有挂过 provider。它**不记** `seq`、哈希、时间戳；`seq` 只作为 U6 里「逐行读账本」时的相邻关系间接出现。

**后置条件**，逐条都是关系或承诺而非期望值：

| 动作 | 断言 |
|---|---|
| `Raise` 一个没人占的良构地址 | 一定被接受，且落一条 `building_created` |
| `Raise` 一个已被占的地址 | 一定被拒，且 `code == "E_INVALID_ARGS"` |
| `Raise` 一个含 `.sprawling` 段的地址 | 一定被拒，且 `code == "E_INVALID_ARGS"` |
| `Work` 而城或该楼停摆 | 一定被拒，且 `code == "E_GATE_DENIED"` |
| `Work` 而未停摆且无 provider | 一定被拒，且 `code == "E_CONFIG_INVALID"` |
| `Seize`（`Takeover`） | 一定被拒，且 `code == "E_WIRE_MISMATCH"`，且 `recovery` 非空 |
| `Look`（`CityView`） | 答里的楼集合恰是模型记的那一套 |
| 每一步之后 | 账本的 `seq` 逐行严格加一，无空洞无重复 |

**三条守序（guard order）被显式钉住**，因为它们是用户看得见的差别：停摆压过配置（`E_GATE_DENIED` 而不是 `E_CONFIG_INVALID`），地址良构压过占用，以及——**对派活而言**——停摆压过地址良构。一个把前两条调换了的实现会在城停摆时叫人去挂 provider——恢复建议指向一件与真实原因无关的事，而恢复建议正是 `AxError` 三段式承诺里的第三段。

第三条是**量出来的，不是想出来的**，而且它纠正的是模型而不是产品。第一版 `refusal` 把 `Work` 的地址判据排在停摆之前，定向对抗随即拿出 `Stop City` 后 `Work ".sprawling/books"` 这条反例。四次直接测量说明产品是自洽的：

| 条件 | 码 |
|---|---|
| 派活，未停摆，保留地址 | `E_INVALID_ARGS` |
| 派活，未停摆，普通地址 | `E_CONFIG_INVALID` |
| 派活，已停摆，保留地址 | `E_GATE_DENIED` |
| 立楼，已停摆，保留地址 | `E_INVALID_ARGS` |

停摆是**派活**这件事最外层的那道门，而立楼不是派活，所以停摆盖不住它。两个次序都有道理，产品选了其中一个并且到处一致；模型断言一个它从未量过的次序，那是模型在发明规则。现在这个次序被钉住了，谁把它调换都会当场判红。

**定向对抗**（U4）：先用两个具体动作把世界推到「有一栋楼」，再 `anyActions_` 生成任意前缀，`action (Stop City)` 插入那一次停摆，`anyActions_` 生成任意后缀，最后以 `failingAction (Work …)` 收口——那一步必须失败，且必须以 `E_GATE_DENIED` 失败。后缀里可能出现 `Resume`，所以收口前先读模型状态，只在仍然停摆时才断言。这是本目录存在的核心理由。

**「拒绝」不是「期望输出」**。模型断言的是**哪一种失败**，从不断言任何被计算出来的值：错误码由 `AGENTS.md` 定为门的契约的一部分，钉住它钉的是产品对调用方的承诺，不是对某条规则的重算。

## 11 边界枚举

| 边界 | 处理 |
|---|---|
| 二进制不存在 | `discover` 返回 `Nothing`，测试树整体标记为 skipped，退出码 0 |
| 端口全被占 | 47100–47115 十六个口由一个 `MVar` 池独占出借；连四次都被外面的进程占住才抛出并说明——这是环境问题，不是产品缺陷 |
| `serve` 起不来 | 探活在 10 s 内拿不到 `city_view` 即抛出，附上它的 stderr |
| 账本只有创世一行 | U6 的相邻断言在少于两行时平凡成立 |
| 轨迹里出现 Windows 路径分隔符 | 全程走 `FilePath`，不手拼字符串 |
| 进程没被杀干净 | `withGround` 用 `bracket`，异常路径也走 `terminateProcess` |
| 同一 `IdemKey` 用两次 | 模型每个动作各铸一个新的；重放同一个由 `keyUsedTwice` 单独断言，见 §4 第三个发现 |

## 12 错误处理

本目录没有「恢复」这个概念：断言不成立就是发现，发现就该停下并交付一个 Rust 回归测试。三件被当作错误处理的事：**门的形状变了**（JSON 解析失败）、**场地起不来**（端口或进程）、**用法错**（退出码 2）。它们都抛异常，因为继续跑只会把新形状当成拒绝，从而把红的测成绿的。

## 13 依赖选型

| 依赖 | 为什么 |
|---|---|
| `quickcheck-dynamic` | `StateModel` 与 `RunModel` 的类型分界**就是黑盒边界**，由类型强制而不靠自律；且它能写有方向的对抗场景 |
| `QuickCheck` | 上游要求 |
| `aeson` | 门讲 JSON。手写 JSON 解析器等于在这里养第二个 bug 源 |
| `process`、`directory`、`filepath`、`temporary` | 起进程、造场地 |
| `tasty` + `tasty-quickcheck` + `tasty-hunit` | 把六组性质编成一棵可选择运行的树 |
| `bytestring`、`text`、`containers`、`mtl` | 基础件 |

**不引入**：任何 FFI、任何绑定 Rust 类型的东西、任何需要改 Rust 代码才能工作的东西、任何 HTTP 服务端（假 provider 会让本目录变成第二个 gateway 实现）。

## 14 硬编码声明

| 硬编码 | 意图 | 后续影响 |
|---|---|---|
| 账本布局 `.sprawling/ledger/*.jsonl` | 敌意动作要按它找到文件 | 该布局改变时本目录判红，属预期 |
| 端口 47100–47115 | 避开常用段，又不需要 `network` 依赖；组串行跑，十六个是给外部占用留的余地 | 冲突时报环境问题 |
| 静默窗口 250 ms | 模型驱动的每个动词都答在 1 ms 内（实测），250 ms 是三个数量级的余量；30 s 那版让每个动作各付满一个窗口，全套跑不完 | 模型开始走慢路径（挂 endpoint）时必须同步改 |
| 三个地址 `acme` / `beta` / `gamma` | 固定的演员表，让反例可读 | 加人时同步改 `Regression.render` 的模板 |
| 模板 `minimal` | 两个模板里不带保密约束的那个 | 要测 `confidential` 时它进模型 |
| 环境变量 `SPRAWLING_BIN` | 二进制位置的唯一入口 | 由 justfile 提供 |

## 15 影响面

对 Rust 侧的影响**必须**恰好为零：不改 `Cargo.toml` 的 members，不进 `cargo deny` 的依赖图，不参与 `xtask length` 的行数，不进 `xtask modmap` 的模块表。唯一的交汇点是 `crates/sprawling/tests/from_adversary.rs`——它由本目录渲染、由 `cargo test` 编译，两侧任何一方漂移都会让某一侧变红。

**一处需要确认的**：`xtask release` 拒绝「引用了本机文件的文件」，`xtask header` 要求每个 `.rs` 带 MPL 抬头。本目录不含 `.rs`，且只引用仓内相对路径与环境变量名，两道门都不适用。

## 16 测试与约束

按「坏得越早越省时间」排序：渲染对拍（U5，毫秒级）、门的契约（U1，含 §4 那条退出码性质）、随机轨迹（U3）、定向停摆（U4）、账本自洽（U6）、磁盘的三句谎话（U2），最后是两个按缺陷命名的组。共 13 条，一整套 22 秒（实测，本机）。约束是 §2 第 5 条——**咬得动**必须被演示过，而不是被相信。

**整棵树串行跑（`NumThreads 1`）。** 一座被端起来的城占着一个端口、一个目录与一条历史，两组同时跑就三样都争。实测过的后果不是变慢而是**换城**：输的那一边城绑不上端口退了出去，它自己的探活却在同一个口上接到了赢的那一边的城，于是一整条轨迹跑在别人的历史上。它把当时还开着的那个缺陷测成了绿的——一个答案取决于哪个线程赢了的对手，比没有对手更坏。

## 17 文档同步

1. 本文。
2. `ARCHITECTURE.md` §11 的验证层表——本目录是 V9 之外的一层，记为 V10，并写明它不是门。已做。
3. `AGENTS.md` 的命令表（`just adversary` 一行）。已做。
4. `justfile` 的 `adversary` 配方。已做。
5. `.gitignore` 的 `adversary/dist-newstyle/`。已做。
6. `.github/workflows/adversary.yml`——定时任务，永远不进 `check`。已做。

---

*本文档采用 MPL-2.0。*
