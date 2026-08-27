# xtask-SPEC.md — 构建门（gates）

> crate：`xtask`（工作区成员，不占产品拓扑）。本 SPEC 先于代码存在；实现不多不少地遵守本文。
> 章节骨架：十七节（「两个设计」置于接口先行后，「模型体验」置于测试与约束后）。

## 1 需求拆解

把 ARCHITECTURE.md 的冻结面变成机器检查，每一件可独立完成、独立验收：

| 单元 | 一句话 |
|---|---|
| header | 每个 `.rs` 前五行恒为 MPL-2.0 通告加版权行 |
| lexicon | 禁用词命中即红；数据面 `xtask/lexicon.toml` |
| modmap | `crates/**/src/**/*.rs` ↔ ARCHITECTURE.md §6 模块表一一对应；状态列一致性；索引文件零逻辑 |
| depmap | crate 依赖边 ⊆ §2 depmap 块；`pub trait` 仅现于 §3 缝清单文件 |
| guard | 触及门自身的提交必须携 `Verdict:` 尾注 |
| zerojs | 仓库无 JS/TS 源文件；我们的命令面（justfile／CI 步骤／build.rs／*.sh）不调用 npm/node 族 |
| spec | 生成 `<crate>-SPEC.md` 骨架（Daily Loop 的 `just spec`） |
| secret（S2.12 上线） | 全仓＋夹具扫 secret shape（判定复用 `kernel::secret::scan`，无内联豁免）；兼查 `Sealed::expose` 调用点白名单 |
| specalign（S2.12 上线） | kernel 枚举 ↔ kernel-SPEC §8-1／§8-4 表逐 variant：消费真 enum（AxCode::ALL／EventKind::ALL）对表作证，计数、归属、carrier／窗类逐项同 |
| apisync（S2.12 上线） | 双断言：①基线新鲜——`cargo public-api` 实时面与已提交基线逐行同；②同集变更——基线文件变即要求同 crate SPEC 同集被触 |
| badge（P5 上线） | 体积徽章由 `budget` 的读数渲染成 `docs/badges/*.svg`；徽章陈旧＝`budget` 门红（不新增门） |
| length（R2.20 上线） | 一个生产函数不得长过 `budgets.toml` 的 `function_length`（今为 200 行）；尺寸以 `syn` 解析后的 span 量得 |
| gates | 顺序跑全部门，聚合报告，任一违规即退出码 1 |

### 门禁针对的 LLM 失效模式（本 crate 存在的理由）

| 失效模式 | 拦截门 | 机制 |
|---|---|---|
| 顺手新建文件／utils 沼泽 | modmap | 模块表是封闭清单，表外文件即红 |
| 声称完成但状态未翻转 | modmap | 文件存在而状态＝未建 → 红（完成四件套的机器面） |
| 逻辑漏进 lib.rs／索引文件 | modmap | 纯索引文件只许注释、属性、mod、use |
| 偷加依赖边、绕过分层 | depmap | 实际边 ⊆ 文档边；kernel 恒零内部依赖 |
| 娱乐性抽象（无第二实现的 trait） | depmap | `pub trait` 只许出现在缝清单文件 |
| 放宽 lint／改门过门／删表行 | guard | 门自身变更必须携用户 verdict 尾注 |
| 词汇漂移、自造同义词 | lexicon | 附录 A 禁用词的机器子集，命中即红 |
| 忘记许可头或版权行 | header | 五行逐字节比对 |
| 引入 JS/TS 或 node 构建步 | zerojs | 扩展名扫描＋命令面 token 扫描（C1） |
| stub／todo!／unwrap 蒙混 | （不在本 crate）workspace lints | clippy deny 已覆盖，本 crate 不重复 |
| 一个函数里塞进整条流程（模型最常见的结构失效） | length | 超过行数预算即红，报出函数名、行数与预算 |

## 2 验收标准

- 每门在违规夹具上报告非零、在干净仓库上报告零（单测覆盖解析与判定核心）。
- 违规输出恒为 three-part 形：rule｜violation｜alternative，附 `gate` 名与 `file:line`。
- `cargo xtask gates` 在本仓库当前状态全绿；人为注入六类违规（表外文件、lib.rs 加 fn、暗依赖边、缝外 pub trait、禁用词、.js 文件）各能单独触红。
- 全部代码过 workspace lints（无 unwrap/expect/panic/索引/切片/裸算术/as）。

## 3 假设与歧义

- 「注释与标识符扫描」简化为整行子串扫描：中文禁用词只会出现在注释与文档，英文禁用词不构成合法标识符片段。误伤由 `lexicon-ok:` 行内豁免兜住。
- guard 本地默认只查 HEAD 一枚提交（工作树未提交的改动不查——门只对可判定对象作证）；CI 以 `--range` 查全推送区间。
- **guard 区分「门怎么判」与「门产出了什么」（P3.01，用户裁定）**。`xtask/api-baselines/` 不在保护面内，而 `xtask/src/apisync.rs` 仍在。理由是两道门曾经互相矛盾：`apisync` 命令公开面一变就跑 `cargo xtask apisync --write`，而那就是写进 `xtask/`；两条同时遵守，等于**每一次公开面变更都要一次裁定**，而一个次次都要签的字会贬值成仪式。重生一份 baseline 放松不了任何东西：`apisync` 仍然拿它与实时 API 逐行比，且 diff 就在提交里给评审人看。**这条不得推广到其它目录**：判据是「该文件由门自己生成且被门自己校验」，不是「改起来麻烦」。
- 附录 A 中语境依赖的禁用词（如 session 指本城运行时、建筑指项目时）不入机器数据面，由评审执行；lexicon.toml 内以注释记录此边界。

## 4 现状分析

空仓库（Stage 0 空壳）。无既有实现可比对；性能无关紧要（全仓扫描 <100ms 量级即可）。

## 5 权威信源

硬化十七条；门表（AGENTS.md）；施工协议（AGENTS.md）；MPL 头全文（`LICENSE`）；退役词全集（`xtask/lexicon.toml`）；ARCHITECTURE.md §3（depmap 块）、§4（缝清单）、§12（模块图列契约）。

## 6 命名统一

gate／Violation／rule／violation／alternative（three-part refusal 的施工侧同构）；模块名与子命令名一致：header、lexicon、modmap、depmap、guard、zerojs、spec、gates。

## 7 模块边界

一门一文件：`main`（分发）｜`report`（Violation 与渲染）｜`walk`（确定性文件遍历）｜`header`｜`lexicon`｜`modmap`｜`length`｜`depmap`｜`guard`｜`zerojs`｜`secret`｜`specalign`｜`apisync`｜`spec`｜`badge`（渲染与陈旧判定，被 `budget` 调用）。

**length 门的形状属于 modmap 而不属于自己**：形状列的解析只住 `modmap::shapes`，因为模块表只应有一个读者——列格式一变，只有一处要改。

**本模块不做什么（否定式三条）**：不做 color（S4 随 web::theme 启用，届时增列）；不修改任何被检文件（门只判不改；唯二例外＝spec 只新建不覆盖、`apisync --write` 只重写基线文件）；不缓存扫描结果（每次全量重扫——确定性优于速度）。

**secret 门细则**（S2.12）：扫描面＝仓内全部文件（含 fixtures／语料），排除隔离区 local/、.git、target；判定器＝`kernel::secret::scan`（xtask 依赖 kernel，工作区成员不占产品拓扑，合法）；命中只报文件＋偏移＋长度，恒不回显字节；无内联豁免（豁免口会被注入内容利用）。兼查：`crates/*/src/**` 内 `.expose(` 调用点白名单＝kernel/src/secret.rs（定义处）、gateway/src/endpoint.rs、gateway/src/native.rs；命中即红。自测纪律：扫描器自身测试的高熵样本在源码中必须拆段拼接，不留可扫描的完整字面量。

**已复核字面量表（P3.05 增）**：判定器恒不改——它的活是在入口捕获一切像钥匙的东西，那里误报不要钱；**本门问的是另一个问题**「这里是不是提交了一份凭证」，那里误报要一次构建。故门内持一张 `NOT_CREDENTIALS` 精确字面量表，逐条写明它是谁、为什么不可能是凭证。三条纪律：①**整串精确匹配**——带前缀或后缀的更长 token 仍是命中，故没人能靠戴一个已复核的名字混过去（一条断言钉这件事）；②**表住门里而不是站点上**——注释式豁免是注入内容能写的洞，这张表不是；③表在 guard 保护面内，增一条即须 `Verdict:` 尾注。首条：`CanvasRenderingContext2d`（`web_sys` 的 2D 画布类型，24 字节且含数字，故触发混合字母表规则；`crates/web/Cargo.toml` 的 feature 与 `web::city_view` 的绘制侧各出现一次）。

**apisync 门细则**（S2.12）：基线集＝存在 `<crate>-SPEC.md` 的产品 crate（SPEC-first 即同步契约面；现在＝kernel/memory/runtime）；基线住 `xtask/api-baselines/<crate>.txt`，由 `cargo xtask apisync --write` 生成（`cargo public-api -p <crate> --simplified`，缺省 feature＝dev-only feature 面不入基线，台账已豁免）；断言①实时重算与基线逐行同（工具链缺失＝fail-closed 报装机指引，不静默跳）；断言②提交区间内基线文件变 ⇒ 同 crate SPEC 同集被触（git 面，复用 guard 的区间语义：本地缺省 HEAD，CI --range）。两断言合成链：API 变→①逼基线更新→②逼 SPEC 同集。cargo-public-api＋nightly 为环境前置（已装，2026-08）。

## 8 接口先行

```rust
// 每门同一形状（判定函数的施工侧同构）：
pub(crate) fn check(root: &Path) -> Result<Vec<Violation>, XtaskError>;
// guard 例外多携参数：
pub(crate) fn check(root: &Path, range: Option<&str>) -> Result<Vec<Violation>, XtaskError>;

pub(crate) struct Violation {
    gate: &'static str,      // 哪道门
    location: String,        // path 或 path:line
    rule: String,            // 规则（引 ARCHITECTURE 节号或 SPEC 章号）
    violation: String,       // 违反点
    alternative: String,     // 合规替代
}
```

退出码：0 全绿；1 有违规；2 用法或内部错误。

## 8.5 两个设计

**A（选中）：文档即数据面**——modmap/depmap 直接解析 ARCHITECTURE.md 的表与围栏块。杠杆：单一权威，改表即改门，无迁写步骤；缝的位置在文档解析函数，可单测。
**徽章 A（选中）：入库的静态 SVG，由 `budget` 的读数渲染。** 杠杆：本仓库是私有库，托管徽章服务读不到里面的数；且产品承诺「不依赖任何托管服务」，一个每次打开 README 都去请求第三方的图片，正好推翻它上方那句话。离线可看，私有可看，无第三方流量。
**徽章 B（落选）：shields.io 的 dynamic/endpoint 徽章。** 只要仓库私有就取不到 JSON；即便将来公开，也是把「这个项目有多大」这件事的呈现权交给一个外部服务。
**徽章 C（落选）：手写数字。** 四期文档里每一处手写的数都至少陈旧过一次——这正是本条存在的理由。

**B（落选）：独立 TOML 清单**（xtask/modules.toml 等）——解析更稳，但立刻造出「文档表 vs TOML」第二权威，需要第三道门看守二者同步；违反「一个事实只住一处」。落选理由记此，翻案条件：markdown 表解析在两期内出现 ≥2 次误解析事故。
（lexicon 例外地用 TOML：禁用词是数据不是结构，必须有仓库内数据面。）

## 9 工作流程

`cargo xtask <gate>` → 定位仓库根（`CARGO_MANIFEST_DIR` 的父目录）→ 读数据面（ARCHITECTURE.md／lexicon.toml／git）→ 纯函数判定 → 渲染违规 → 退出码。`gates` 依序跑全部机器门（header→lexicon→modmap→depmap→zerojs→secret→color→budget→specalign→apisync→release→guard），聚合后统一渲染。**门数只住 `gates::COUNT`**，且它就是那张数组的长度类型参数——数目与清单相隔一个 token，故不可能各说各话；文档要说门数就去读它，不再自己写一个。

## 10 实现逻辑

1. **walk**：手写递归（不引 walkdir），跳过 `.git`／`target`／`node_modules`，输出按路径字符串排序——报告顺序确定，diff 可比。路径统一正斜杠（Windows 反斜杠归一），因为模块表以正斜杠书写。**隔离区**：仓库根 `local/`（gitignore，恒不入库）存 Handoff 与本机备忘；从仓库根扫描的三门（header／lexicon／zerojs）排除它——门只对入库对象作证，非入库物可引用历史词汇与本机路径。modmap／depmap 只扫 `crates/`，嵌套的 `crates/**/local` 仍被封闭清单咬住，无洞。
2. **modmap**：模块行判据＝竖线表行、第 2 列以 `crates/` 开头以 `.rs` 结尾、第 1 列含 `::`、恰六列、第 6 列 ∈ 状态枚举——这组条件把 §3 缝表（四列）与 §10 卡（清单行）天然排除。双向对账：表有文件无（状态≠未建 才要求在盘）；盘有表无（lib.rs 与索引文件豁免）；盘有且状态＝未建 → 「状态未翻转」。索引文件判据：文件名去 `.rs` 后与同目录某子目录同名，且该子目录内有表内文件。
3. **depmap**：§2 围栏块 ```` ```depmap ```` 为机器权威；`cargo metadata --format-version 1 --no-deps` 输出经 serde_json::Value 读取；只查 normal＋build 依赖（dev 依赖留给测试自由）。子集断言而非相等断言——空壳期合法。
4. **guard**：`git rev-list` 取区间（缺省 HEAD 单枚；无提交则跳过并说明），`git diff-tree --root` 取动过的文件，`git show -s --format=%B` 取信息；保护路径命中或 ARCHITECTURE 模块行被**移除**而信息无 `Verdict:` 行首 → 红。移除的定义是路径级的：某 `crates/**.rs` 路径出现在删除行（`-|`）且不出现在任何新增行（`+|`）——状态翻转在 diff 里是「删一行加一行」，它是最高频的合法编辑，若被误判为删行索 verdict，门就在训练绕门习惯。
5. **vocabulary（R1.15，挂在 lexicon 门下）**：两条断言，各修一种第二权威。①**退役词必须指向被定义过的词**——`lexicon.toml` 说哪种说法作废，`docs/glossary.md` 说该用哪个词，此前无人让二者对账，于是一条退役词可以指向一个词汇表从未定义的名字，而照门的建议改词的人会落到一个没有释义的词上。判据宽一格：replacement 命中任一词汇表**粗体词**或含 `.md`（指向一份文件也是一种定义）。②**能被机器数出来的数不由文档手写**——产品面文档里每一处手写门数都至少陈旧过一次（四份文档写「ten gates」而实际跑十二道）。故读 `gates::COUNT` 与文档对账，中英两种写法（`ten gates`／`十二门`）各认一组，且**刻意只认已经烂过的那几种形状**：一条会猜的规则就是一条会在别的正文上乱咬的规则。
6. **release（P4.14 上线，P5.05 增第三条断言）**：公开树**由过滤生成**而不由手工挑选，分类是一条**封闭的前缀规则**（`is_scaffolding`）；未被规则点名的一律归产品面——**失败方向是故意的**：未分类的文件出现在产物里会被人看见，反过来则悤声消失。三条断言：①公开树上零脚手架路径；②产品文档不得链向或在正文里点名脚手架（无链的「去看 SPEC」最好写也最难发现，故扫全文而不只扫链接）；③**任何发布文件不得携家目录路径**（`machine_path`）。第三条的口径是**隐私而非整洁**：`/tmp`、`/etc`、`C:/windows` 是关于一类机器的事实，而且「绝对路径被拒」那三条测试必须写出一个绝对路径——典型反例先咬住的正是它们，故规则收窄到家目录形状（`:\users\`／`:/users/`／`/home/`／`/root/` 等七种，大小写不计）。扫描面是**全部可读成文本的发布文件**，不只 `.md`：源码与清单里的硬编码家目录更坏而不是更好。报告只截二十字符，因为把整行引进 CI 日志就是把它再公开一次；文件自豁免（同 secret／color 两门的先例：写不出不包含待检形状的检测器）。
7. **badge（P5）**：读数与 `budget` 同源（同一 `measure()`），故不存在第二个数字权威。每个可称重的 gated 行若在 `budgets.toml` 里带 `badge_label`，即渲染一张 `docs/badges/<行名>.svg`。三条纪律：①**颜色不自选**——灰阶取自 `crates/web/src/theme.rs` 的 `GRAY_RAMP`（复用 color 门已有的解析器），OKLCH→sRGB 的换算在此一次算清，因为 SVG 要被任意浏览器渲染，而 `oklch()` 的支持面不覆盖旧版；墨色恒不低于 `INFORMATION_FLOOR`，一条断言钉住。②**平台自报**——二进制体积逐平台不同，故 `badge_platform` 指名哪台机器有权刷新它，别的平台既不写也不判，否则三个平台会互相覆盖同一个文件。③**陈旧即红**——`budget` 门在能称重时比对已提交的 SVG 与当场渲染的 SVG，不同即红并给出 `cargo xtask badge --write`；称不出（本机没构建产物）则沉默，与该门既有的沉默口径一致。`just dist` 末尾调用它，于是发一次 release 就刷新一次，没有人需要去改一个数字。
9. **length（R2.20）**：尺寸的单位是**生产函数**，不是文件——按文件计的任何诚实阀值会在四个 crate 里同时点燃八处，那是一个工程而不是一道门。**扫描面**：`crates/*/src`、`xtask/src`、`citysim/src`；`tests/` 与 `benches/` 不在内，因为测试代码本就允许放松约束（AGENTS.md）。**三类不量**：① 带 `#[cfg(test)]` 的项（它标的是**一个项**而不是文件剩下的部分）；② 带 `#[component]` 的函数（Dioxus 组件，函数体即标记，没有可跟的步骤）；③ 模块表形状列为 `data` 的文件（ARCH §9 形状 6：数据而无分支）。**三类豁免都取自已有权威**（属性、模块表），而不是新建一张名单——一张名单就是一个可以您您变长的豁免口。形状列由 `modmap::shapes` 交出，与 modmap 共用同一个表解析器。
10. **报告**：three-part 渲染，与产品的 Gate 拒绝同构——施工者被拒时拿到的也是「规则｜违反点｜替代」，不是一句 fail。

## 11 边界枚举

词汇表粗体词一个都解析不出（表结构变了→ Doc 错误而非静默通过）；空仓库（无提交→guard 跳过）；表行路径重复；状态列取值非法；围栏块缺失（→ Doc 错误，非零违规）；CRLF 行尾（比对前 trim `\r`）；非 UTF-8 文件（lossy 读，不 panic）；merge 提交（diff-tree -r 照常）；initial commit（`--root`）；**命令面的注释行不参与 zerojs token 扫描**（`#`／`//` 打头的行不可执行，把它们当命令判是把说明文字当成了行为——首跑即被自身 CI 注释命中的实例回填此条）。

## 12 错误处理

`XtaskError`（thiserror）：`Io{path}`｜`Doc{file,msg}`（数据面不可解析）｜`Cmd{cmd,msg}`（git/cargo 调用失败）｜`Usage`。数据面坏＝退出码 2（门自身故障），不伪装成 0 或 1——门坏了必须显性，静默通过是门的最坏失效。

## 13 依赖选型

serde＋serde_json（cargo metadata 解析；工作区已钉）；toml（lexicon 数据面；xtask 独用，不入产品面）；thiserror（工作区已钉）；kernel（S2.12 起：secret 门复用 `kernel::secret::scan`，一个判定一个家）。不引 walkdir/regex/clap：手写遍历十几行；判定用子串与前缀即可（C12 对 regex 的敏感面在 kernel，此处一并回避）；子命令分发一个 match 足矣。

**syn 与 proc-macro2**（R2.20，`syn` 开 `full`，`proc-macro2` 开 `span-locations`）：**量一个 Rust 函数从哪行到哪行是一个解析问题，不是一个数括号问题**。本卡先写了一个按行数括号的探针，一小时内撞上三个计数错误，**每一个都产出了一张错的违规名单**：① `#[cfg(test)]` 被当成文件截断点，于是 `assembly.rs` 第 5046 行一个测试助手以下的函数全部隐形（`pub async fn serve` 就在里面）；② `'{'` 这样的字符字面量被当成开括号，`detect` 于是从 43 行变成 266 行；③ 跨行字符串（`"… \` 换行 `…"`）同理，`malformed` 从 6 行变成 230 行。一道量错的门比没有门更坏：它会把人送去拆一个不需要拆的函数。替代方案是手写一个状态扫描器（行注释、可嵌套块注释、转义与跨行字符串、raw string 的 `#` 计数、以及 `'a` 生命期与 `'x'` 字符的区分）——八十行代码养一个第四个计数错误的地方。`syn` 是编译器旁的那个解析器，且已因每一个 derive 宏而在 `Cargo.lock` 里。维护成本：仅工作区工具链，恒不入产品二进制（同 flate2／zip 先例）。

**zip**（P7.02，`default-features = false, features = ["deflate-flate2"]`，净增两个包）：复用 xtask 已有的 flate2 做压缩后端。替代方案是在 justfile 与 CI 里按平台分支调 `Compress-Archive`／`zip`／`tar`，已验证否决：git-bash 携的是 GNU tar，不产 zip，三个平台因此需三段 shell，且本机与 CI 的产物不同源——那正是本轮要关掉的那类差异。维护成本：仅工作区工具链，恒不入产品二进制。

## 14 硬编码声明

行数预算 200 **不**硬编码在门里，它是 `xtask/budgets.toml` 的一行（`[function_length]`）——那份登记表自述是「设计所声明的每一项预算」，而它已经持着非字节的预算（`kernel_mutation` 的百分比、`ledger_append` 的毫秒）。数字的来历写在那一行的注释里，改它受 guard 看守。

MPL 头三行；保护路径清单（xtask/、.github/、deny.toml、Cargo.toml、rust-toolchain.toml、clippy.toml、justfile）；状态枚举四值；JS 扩展名族与 node 命令族。各随其权威变更而改，改动本身受 guard 看守。

## 15 影响面

CI 与 justfile 调用面；ARCHITECTURE.md §6/§2/§3 的表格式即本 crate 的解析契约（列契约已标〔冻〕）。改表格式＝改本 crate。

## 16 测试与约束

单测：modmap 行解析（正例/六列不齐/状态非法/缝表不误伤）；索引文件判定；lexicon 命中与 `lexicon-ok:` 豁免；depmap 块解析；zerojs token 化（`node_modules` 不误伤 `node`）；header 比对（CRLF）；隔离区前缀判定（`local/` 命中、`localx/` 不命中）。约束：全门无网络、无写盘（spec 子命令除外——它只新建不覆盖）；输出顺序确定。

## 17 模型体验

零字节：本 crate 不进任何 Run 的 prefix；施工者只在门红时读到 three-part 报告——边界反馈优于开头说教的施工侧实例。

## 18 文档同步

新增门或改保护路径时：AGENTS.md 的规则表、`docs/CONTRIBUTING.md` §3 同集更新。
