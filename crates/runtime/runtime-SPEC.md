# runtime-SPEC.md

> crate：`runtime`。本 SPEC 先于代码存在；实现不多不少地遵守本文。
> Stage 1 两模块（replay／fork）＋Stage 2 三模块最小形（turn／prefix／handoff，§8-3…§8-5）。
> Stage 3 增章：完备化（§8-6）＋pipeline／offload（§8-7／§8-8）＋watchdog（§8-9）＋clock／catalog／mode（§8-10…§8-12)＋sandbox 缝（§8-13）＋tools 三件（§8-14）。

## 1 需求拆解

| 卡 | 模块 | 一句话 |
|---|---|---|
| S1.10 | `replay` | 离线重演验链：EventRef 第二铸造点；A2 的执行体 |
| S1.10 | `fork` | 分叉前缀：母 Run 事件序列到节点为止的逐字节前缀；A19 的执行体 |
| S2.01 | `turn` | 回合 typestate 四相＋取消边界；形状 5；相内中断编译不过 |
| S2.02 | `prefix` | FrozenSegment 四段＋分段哈希＋易变类型隔离；形状 5＋2 |
| S2.02 | `handoff` | 五段构造点＋resume 消费 Handoff 产新 Run 种子；形状 2 |
| S3.08 | 完备化 | prefix 四段全量（封顶＋截断标注＋跨段去重＋跳过入账）＋断点 ≤4＋Steer 边界消费＋窗口组装入 Assembling 相 |
| S3.09 | `pipeline`＋`offload` | 结果信封三附件＋offload 四不变量（独占有损可还原）＋截断定序 |
| S3.10 | `clock`＋`catalog`＋`mode` | ClockStamp 纯格式化＋渐进披露三类条目＋五 mode 枚举 |
| S3.11 | `watchdog` | 处置面分级（纠正 Steer→停滞→冻结）；判据只从 kernel::stall 来 |
| S3.12 | `sandbox` | 缝（trait）＋wasmtime fuel 生产适配器＋直通/故障两替身；A10 三断言 |
| S3.13 | `tools/` 三件 | exec 三臂／edit 乐观并发／status 十二字段＋ToolBench 按 Effect 过门 |

「replay 只重演不重执行」的 S1 含义：本期重演＝验证链与重建记录序列；入窗重建器（C16/A15）随 prefix 组装（S2+）加入，两卡共用本模块的验证输出。

## 2 验收标准

- A2 演示：对 jsonl 落盘目录与内存行序列各跑一次 verify，逐事件验 prev 链与 seq 连续；任一字节被篡改即拒并报行号。
- A19 演示：从任一节点取分叉前缀，与母 Run 原始行 0..=at_seq 逐字节相同；`at_seq` 越界＝`E_INVALID_ARGS`（恒不静默截到末尾）；母序列不因分叉改变。
- 未知 kind：无 `ig:true` 即拒（方向语义：更新的写方）；带 `ig:true` 的行跳过类型化解读但链照验。
- S3.08：A4——同一 PrefixPlan 两次 build 逐字节同（golden）；A15——prompt_assembled 载荷＋同源文档经 `rebuild_prefix` 重建，四段哈希逐段相同。
- S3.09：A7——offload 往返（替代体≤原件且≤上限；循 rest_path 续读与循 CAS 取回字节一致；外部清理后自 CAS 重物化字节一致；命中既有哈希直引原 Locator）。
- S3.10：A18 零字节——granularity=Off 时打包输出与未接 clock 特性逐字节相同。
- S3.12：A10 三断言结论书——fuel 内成功／未授能力被拒／fuel 耗尽中断（真 wasmtime 上）。
- S3.13：L0×失败注入矩阵；A6 双守（Done 恒携 kind 合法证据，运行时纵深校验）；A8（到限恒 `Completion::Limit`，恒不记完成）。

## 3 假设与歧义

1. **verify 的规范复验**：v1 无升级器链，故对每行断言 `canonical_line(parse_line(raw)) == raw`（写方规范性质）。未来 v>1 经升级器读入后此断言只对原版字节成立——届时随升级器一并改约（本文更新）。
2. **fork 的 run_forked 落账**：事件写入母城 Ledger 由调用方（S2 起 runtime 回合层／citysim）执行；本期 fork 只产 EventDraft 与前缀，不持 Ledger 句柄——保持纯函数形。
3. **同一套重建器**：A15 与 A19 共用 verify 输出；本期重建器＝verified 行序列本身。

## 4 现状分析

空壳。verify 为 O(n) 全量；S1 消费面（测试/夹具/citysim）规模千行级，无性能议题；seq→偏移索引属 S3 memory::index。

## 5 权威信源

Fork 三规则；重放/分叉/幂等；at_seq 越界、未知 kind、崩溃恢复行；kernel-SPEC §8-4/§8-9；memory-SPEC §8-1。

## 6 命名统一

replay、verify、VerifiedLedger、VerifiedLine、fork prefix、`at_seq`。不引入「重播/回放/复演」等同义词。

## 7 模块边界

```
replay ──▶ kernel(event/ledger/error)、memory(jsonl::read_raw_lines)
fork   ──▶ replay(VerifiedLedger)、kernel
turn   ──▶ kernel(ledger/event/error/tool/model)、prefix(FrozenPrefix)
prefix ──▶ kernel(locator::B3Hash/event::Payload/error)
handoff──▶ kernel(locator/event/error)
pipeline ──▶ offload、clock、kernel(tool)
offload ──▶ memory(cas)、kernel(locator)
watchdog ──▶ kernel(stall/completion)
catalog ──▶ kernel(tool)、mode
sandbox ──▶ wasmtime（feature `wasm` 内藏；缝声明恒在）
tools/ ──▶ kernel(tool/version/discard/gate)、sandbox、memory(cas 经 pipeline)
```

**本 crate 本版不做什么（否定式三条）**：
- 不重执行任何效果——verify 恒不调工具、不出网、不写盘。
- 不生成 RunId——新 Run 身份由调用方注入（kernel 禁随机的同一纪律）。
- 不读 projection——重放的唯一输入是 Ledger 原始行（历史只有一份）。

## 8 接口先行（按模块分章）

### 8-1 runtime::replay（S1.10）

```rust
pub enum VerifiedLine {
    Known { record: EventRecord, echo: EventRef },   // echo：第二铸造点产物
    IgnoredUnknown { seq: Seq },                     // ig:true 的未知 kind
}
pub struct VerifiedLedger { /* 私有：lines: Vec<Vec<u8>>, verified: Vec<VerifiedLine> */ }
impl VerifiedLedger {
    pub fn raw_lines(&self) -> &[Vec<u8>];
    pub fn lines(&self) -> &[VerifiedLine];
    pub fn tail_seq(&self) -> Option<Seq>;
}
/// Offline chain verification (A2). Errors carry the failing line number in
/// `subject`. Refuses: v > EVENT_LOG_V (direction-aware), broken prev chain,
/// seq gaps, non-canonical bytes, unknown kind without `ig:true`.
pub fn verify_lines(lines: Vec<Vec<u8>>) -> Result<VerifiedLedger, AxError>;
/// Convenience over a jsonl directory: memory::jsonl::read_raw_lines + verify.
/// 无段目录与空账本在此同形（均得空 VerifiedLedger）——本函数的调用方均自持城根算出路径；
/// 区分二者是「从人那里拿到路径」的一层的事（§11；sprawling-SPEC §12）。
pub fn verify_ledger_dir(dir: &Path) -> Result<VerifiedLedger, AxError>;
```

流程：逐行①envelope 探查（serde_json::Value：v/seq/prev/kind/ig 键）；②v 判向（>EVENT_LOG_V 即 `E_LOG_VERSION_UNSUPPORTED`）；③链续（`chain_hash` 复算对拍 prev，首行对 GENESIS_PREV）；④seq 连续（自 FIRST 起）；⑤kind 已知→`parse_line` 全解＋规范复验＋`to_ref`；未知＋`ig:true`→记 IgnoredUnknown；未知无 ig→`E_LOG_VERSION_UNSUPPORTED`（subject=kind＋行号）。链与 seq 对一切行（含 ignored）成立。

**「没找到要验的东西」与「验过且为空」必须异形，但不在这一层异形**（issue #3）。`verify_ledger_dir` 的四个生产调用方（`fold`、`rebuild_views`、`startup_scan`、`fork`）均自持城根算出路径，而 `JsonlLedger::open` 只建目录、首次 append 才建段：**已开未写的城恰好是一个无段目录**，在此处报错会把一个合法启动打红（`fold` 早已以 `if ledger_dir.exists()` 记下这个状态）。若改成在此报错，四个调用方就各需一份同样的守卫——一条条件四份拷贝。

故判据归给**拿到人输入路径的那一层**：`sprawling replay <ledger-dir>` 先问 `memory::ledger_segments_at`，一段都没有就报 `E_PATH_NOT_FOUND`（sprawling-SPEC §12）。先例取自本仓库：`xtask guard` 在无提交时说 `no commits yet, nothing to judge`，而不说通过。**空账本本身仍然合法**：`verify_lines(vec![])` 照旧返回空 `VerifiedLedger`。

### 8-2 runtime::fork（S1.10）

```rust
/// Byte-identical fork prefix (A19): raw lines 0..=at_seq of the verified
/// mother sequence. `at_seq` past the tail is E_INVALID_ARGS, never a
/// silent clamp to the end.
pub fn prefix(mother: &VerifiedLedger, at_seq: Seq) -> Result<Vec<Vec<u8>>, AxError>;
/// The run_forked draft for the city Ledger. Caller supplies the new run
/// id and clock reading; fork itself is pure.
pub fn fork_draft(from: RunId, at_seq: Seq, new_run: RunId, t: TimeMs, who: String)
    -> Result<EventDraft, AxError>;      // data = {"from": …, "at_seq": …}
```

### 8-3 runtime::turn（S2.01；形状 5 typestate 机）

```rust
pub struct Turn<S> { /* run、who、t、state —— 全私有；相内数据在别的相不可表示 */ }
pub struct Assembling(/* 私有 */);  pub struct Calling { /* prefix 哈希 */ }
pub struct ToolWave { /* calls */ }   pub struct Recording { /* refs */ }

#[non_exhaustive] pub enum Interrupt { None, Cancel }    // Steer variant 随 S3 只加（消费方传值不 match，开放无痛）
pub enum PhaseOutcome<Next> { Advanced(Next), Cancelled(TurnCancelled) }
// PhaseOutcome 刻意穷尽：新结局必须逼每个执行器表态，不得掉 catch-all；
// 14.3 的 non_exhaustive 规则辖 wire 冻结枚举，不辖判定输出（verdict 枚举全库同此例）。
pub struct TurnCancelled { /* refs：含 cancel_received —— 私有，getter 取 */ }
pub struct TurnReport { /* refs、model_returned_ref、wave_len —— getter 取 */ }

impl Turn<Assembling> {
    pub fn begin(run: RunId, who: String, t: TimeMs) -> Turn<Assembling>;
    /// Boundary 1 (组装前). Cancel here consumes before any model bytes.
    pub fn assemble(self, interrupt: Interrupt, ledger: &mut dyn Ledger, prefix: &FrozenPrefix)
        -> Result<PhaseOutcome<Turn<Calling>>, AxError>;          // 产 prompt_assembled
}
impl Turn<Calling> {
    /// Boundary 2 (provider 调用前).
    pub fn call(self, interrupt: Interrupt, ledger: &mut dyn Ledger, model: &mut dyn Model,
                policy: &BuildingPolicy) -> Result<PhaseOutcome<Turn<ToolWave>>, AxError>;
                                                                  // 产 model_called＋model_returned
}
impl Turn<ToolWave> {
    /// Boundary 3 (工具执行前). One wave, serial in S2; per call
    /// tool_called + tool_result. Invoker is a plain closure — no second
    /// dispatch trait until a second consumer exists (S3 catalog).
    pub fn execute(self, interrupt: Interrupt, ledger: &mut dyn Ledger,
                   invoke: &mut dyn FnMut(&ToolCall) -> Result<ToolOutcome, AxError>)
        -> Result<PhaseOutcome<Turn<Recording>>, AxError>;
}
impl Turn<Recording> {
    pub fn record(self, interrupt: Interrupt, ledger: &mut dyn Ledger) -> Result<PhaseOutcome<TurnReport>, AxError>;
}
```

- **相变函数携 `&mut dyn Ledger`，相内字段私有**；无返回既往相的方法；跳相／相内取消／字面量构造中间相，三者编译不过（trybuild，S2.01 随卡、S2.11 入全集）。
- **取消只在边界**：每相变函数首参即边界快照；命中 Cancel → 追加 cancel_received → 返回 Cancelled（回合终止，后续 handoff_written＋run_frozen 归执行器）。相内无任何中断入口＝A9 的结构化一半；另一半（事件序断言）在 citysim。
- **四取消点**：组装前／provider 调用前／工具执行前／派生前，四点全住本模块。第四点由 `Turn<Recording>::record` 收边界快照，故 `record` 与前三相同形——收 `Interrupt`、答 `PhaseOutcome`。它买到的是别处买不到的一件事：**一个回合把活派下去之后、子 Run 起来之前，仍停得住**；`calls_made == 0` 的收尾回合尤其如此，那一刻在第四点之前根本没有下一个边界。
- **model_called 载荷**：segments 哈希（与 prompt_assembled 同源）；model_returned 载荷＝message＋calls 数。S3 接真 dialect 时只加字段。
- **工具波 S2 串行**：并行执行串行入账（确定性 5）属 S3 并发波；接口不预留并发参数，入账序＝calls 序。

### 8-4 runtime::prefix（S2.02；形状 5＋2）

```rust
pub enum SegmentSlot { City, Building, Resident, Run }   // 四段恒四，穷尽不扩
pub struct FrozenSegment { /* slot、bytes、hash —— 私有 */ }
impl FrozenSegment {
    /// The only way in: static bytes from frozen sources. Volatile types
    /// (TimeMs, usage, signals) have no conversion into this type — the
    /// absence of those impls is the isolation guarantee (15.3-4).
    pub fn new(slot: SegmentSlot, bytes: Vec<u8>) -> FrozenSegment;   // hash＝B3Hash::digest
    pub fn slot(&self) -> &SegmentSlot;  pub fn hash(&self) -> &B3Hash;  pub fn bytes(&self) -> &[u8];
}
pub struct FrozenPrefix { /* 四段 —— 私有 */ }
impl FrozenPrefix {
    /// Slot order is the type: city, building, resident, run. A mismatched
    /// slot in any position is E_INVALID_ARGS (fail-closed, no reorder).
    pub fn assemble(city: FrozenSegment, building: FrozenSegment,
                    resident: FrozenSegment, run: FrozenSegment) -> Result<FrozenPrefix, AxError>;
    pub fn segment_hashes(&self) -> [B3Hash; 4];
    pub fn prompt_payload(&self) -> Result<Payload, AxError>;   // prompt_assembled 载荷：逐段 {slot, hash, len}
}
```

- 段序即缓存经济：类型把四段位置写死，断点与各段上限属 S3 完备化（只加字段）。
- 分段哈希经 `B3Hash::digest`（kernel 唯一哈希产地）；A4（同输入同字节）由 golden 断言，A15 重建器随 S3。
- trybuild 反例：`FrozenSegment::from(TimeMs)`／把 TimeMs 传进 assemble —— 无转换路径，编译不过（ClockStamp 等类型落地后同规逐个加反例）。

### 8-5 runtime::handoff（S2.02；形状 2）

```rust
pub struct Handoff { /* must_read、overview、progress、context、next_step —— 私有 */ }
impl Handoff {
    /// Sole constructor: must-read non-empty; every
    /// entry is an already-parsed Locator by type. Five sections always
    /// present; prose quality is the probe's business (P1), not the type's.
    pub fn new(must_read: Vec<Locator>, overview: String, progress: String,
               context: String, next_step: String) -> Result<Handoff, AxError>;   // 空 must_read → E_INVALID_ARGS
    pub fn must_read(&self) -> &[Locator];  pub fn payload(&self) -> Result<Payload, AxError>;  // handoff_written 载荷
}
pub struct ResumeSeed { pub run: RunId, pub must_read: Vec<Locator> }
/// Resume consumes a Handoff and mints a new identity — never revives the
/// frozen one (元原则六). The caller supplies the new RunId (kernel 禁随机).
pub fn resume(handoff: &Handoff, new_run: RunId) -> ResumeSeed;
```

- 「下一步」段首列用户指定动作、must-read 规范类机器填：内容约束属生产者（S3 回合层／P2 spine_files），类型只强制结构。
- Run<Frozen> 无解冻：resume 不收 Run 值，只收 Handoff——「旧 Run 醒来」在签名上无法拼写。

### 8-6 S3.08 turn／prefix／handoff 完备化（形状不变，参数长入）

「只加不改」的取义：typestate 四相、边界消费、事件序、私有字段三不变量不动；相变函数的入参按 S3 语义长入（assemble 增 window/tools），消费者（citysim）同集更新。被否替代：平行第二条 call 路径——同一相两个入口即两个权威，落选。

```rust
// kernel::model 增（缝上 canonical 会话类型，kernel-SPEC §8-24 同集改）：
// ChatRequest { system: Vec<SystemBlock>, messages: Vec<ChatMessage>, tools: Vec<ToolDef> }
// SystemBlock { text, cache }；ChatMessage { role, content: Vec<ContentBlock> }；Role { User, Assistant }
// ContentBlock { Text{text} | ToolUse{id,name,input:Payload} | ToolResult{tool_use_id,content,is_error} }
// ToolDef { name, description, input_schema: Payload }；ModelUsage 四整数；StopReason { EndTurn, ToolUse, MaxTokens }
// ModelRequest 增 chat: ChatRequest；ModelReturn 增 usage: Option<ModelUsage>、stop: Option<StopReason>、billed: Option<UsdMicros>

pub struct Window { /* messages: Vec<ChatMessage> —— 私有；执行器持有，逐回合推进 */ }
impl Window { pub fn new() -> Window;
    pub fn push_steer(&mut self, source: &str, text: &str);          // 「user」或「@ID」前缀形
    pub fn push_task_lines(&mut self, task: &str, goal: &str, opening: Opening);  // 首轮，run_started 可重建
pub enum Opening { FromJob, WithPerson }   // P6.03：穷尽两臂，城在写 brief 时已决定
    pub fn push_assistant(&mut self, content: Vec<ContentBlock>);
    pub fn push_tool_results(&mut self, results: Vec<ContentBlock>); // ToolResult 块（pipeline 产出的成品文本）
    pub fn messages(&self) -> &[ChatMessage]; }

pub struct CallShape { pub model: String, pub max_tokens: u64, pub effort: Option<Effort> }
                    // P1.10：三项全部来自选型点，无一项在调用处手写。model 与 max_tokens 解自
                    // 模型目录行（gateway::market::ModelEntry）；effort 解自 kernel::FrozenConfig，
                    // Run 内恒不变——改它就换缓存前缀（理由与出处在 kernel-SPEC §8-22）
impl Turn<Assembling> {
    pub fn assemble(self, interrupt: Interrupt, ledger: &mut dyn Ledger, prefix: &FrozenPrefix,
                    window: &Window, tools: &[ToolDef], shape: &CallShape)
        -> Result<PhaseOutcome<Turn<Calling>>, AxError>;   // Calling 相私持已组 ChatRequest；prompt_assembled 载荷长入
}
// Interrupt 增 Steer { source: String, text: String }：边界消费→追加 steer_received（in-window）→照常 Advanced（不终止回合）；
// 文本回折入 Window 归执行器（它持 Window 与 Steer 原文），呼应「追加在结果末尾」。
// TurnReport 长入：model_content: Vec<ContentBlock>（助手内容）与 wave_results: Vec<ContentBlock>（ToolResult 块）——
// 执行器据此折叠 Window；离线重建同源于 model_returned.data.content 与 tool_result 事件（C16 一致）。
// kernel::ToolCall 增 id 字段（S3.08 同集）：tool_use↔tool_result 对号是两 Dialect 的 wire 硬性要求；
// tool_called 载荷增 id，tool_result 载荷增 tool_use_id。
```

**prefix 四段全量**（新增构建面；既有 FrozenSegment/assemble 不动）：

```rust
pub struct SourceDoc { pub addr: Address, pub bytes: Option<Vec<u8>> }   // None＝缺失或不可读（跳过入账）
pub struct SegmentCaps { pub city: u64, pub building: u64, pub resident: u64, pub run: u64 }  // 字节上限；来源＝调用方（S3 取 STARTUP_BUDGET_TOKENS×4 的四均缺省，住 consts 消费侧不另设常量）
pub struct PrefixPlan { pub city: Vec<SourceDoc>, pub building: Vec<SourceDoc>,
                        pub resident: Vec<SourceDoc>, pub run: Vec<SourceDoc>, pub caps: SegmentCaps }
pub struct PrefixBuild { pub prefix: FrozenPrefix, pub notes: Payload }   // notes＝逐段 sources/skips/truncations（prompt_assembled 载荷入口）
pub fn build_prefix(plan: PrefixPlan) -> Result<PrefixBuild, AxError>;
```

- **首轮不再指向任何东西（P6.03）**：`JOB.md` 的正文已是 Run 段，故 `FULL READ:` 那一行与它携的 `cas:b3-…` 一起取消——城里没有一个工具解析得了内容哈希，而溯源在 Ledger 里已记两遍。`Opening` 的两臂不是排版偏好：被派了一件活的会话与正在和人说话的会话要的第一句话不同，而把人那句话包成 `Task:`／`Goal:` 表单，换回来的也是一张表单。
- **read 的两条路，差别在于谁选的（P6.04）**：**路径是模型选的，故受审**——`Address::parse` 杀穿越，`is_reserved` 杀保留子树（`E_GATE_DENIED`）；**catalog 里的名字是人选的**——楼的阅览室写下它时准入就已发生，故它解到的 skill 可以住在保留空间里。两条路共用一个参数，因为对模型而言它们是同一件事（把一份东西调到眼前）；**先问 catalog** ，一个同名文件不得遮蔽楼已经准入的 skill。
- **`Catalog::expand` 改答 `Expansion { Skill { addr }, Said { text } }` 而不是 `String`（P6.04）**：skill 展开成一个可打开的地址，其余展开成目录自己持有的正文；两者压成一个字符串时，调用方只能拿它去试解析成地址，而一段恰好能解析成地址的正文就会被当成文件打开。这个错误真发生了，是一条红测试拿住的。
- **正文不在 prompt 里，所以交出去而不是拒绝**：`render()` 只写每条的 disclosure，`expansion` 从未进过窗口。最初那版 `read` 对 mode 答「已在你的 prompt 里，没什么可打开」，是错的。
- **它是 `Catalog::expand` 自 S3.10 以来的第一个调用者**：在它之前，一栋楼的阅览室能报出一个 skill 的名字而永远交不出它。
- 跨段去重：同 addr 两段命中只装首次（段序 city→building→resident→run）；后段记 skipped{reason:"duplicate"}。
- 截断：文件超段位余额即截到边界，原处留 ASCII 标记 `[truncated: N bytes]`（prefix 面向英文窗口），恒不静默丢尾；标记字节从段预算先扣。
- 断点：`FrozenPrefix::system_blocks()` 产四块、逐块 cache=true＝断点恒 4＝`CACHE_BREAKPOINTS_MAX`，断点只落段界。
- A15 重建器：`replay::rebuild_prefix(data: &serde_json::Value, resolver: &dyn Fn(&Address) -> Option<Vec<u8>>) -> Result<[B3Hash; 4], AxError>`——从 prompt_assembled 载荷（逐源 {addr, kept, marker, dropped}）与同源文档重算逐段哈希对拍；resolver 以 Address 取文（钉版 oid 级解析随 checkpoint 接入升级，接口不变）。截断标记与拼接规则的唯一权威住 prefix.rs（pub(crate) 常量），replay 同 crate 复用不另拷。
- E_TOOL_OUTCOME_UNKNOWN 补写面（本卡随 replay 交付）：`replay::dangling_tool_calls(&VerifiedLedger) -> Vec<(RunId, Seq)>`（tool_called 后邈无同 run 的 tool_result 即 dangling）＋`replay::outcome_unknown_draft(...) -> EventDraft`（补写的 tool_result，携 E_TOOL_OUTCOME_UNKNOWN 错误体）；消费者＝resume 路径（S4 serve；台账登记）。
- handoff：S2 形已全（五段＋构造点＋resume 消费），本卡零改动；「下一步段首列用户指定动作」属生产者纪律（S3 执行器／P2 spine_files），类型不另加钩。
- 第四取消点（派生前）：S2 推迟，理由是当期无派生生产者，提前落地＝死入口＋不可测。`card-P1.01` 的 `collab::delegate_tool` 是那个生产者，故本点随 `card-P1.02` 落地：`SafePoint::BeforeSpawn` ＋ `Turn<Recording>::record(interrupt, ledger)`，装配层在 `Completion::Cancelled` 时清空派生台，**被取消的 Run 一件活也交不下去**。

### 8-7 runtime::pipeline（S3.09；形状 1＋组装处）

```rust
pub struct PackContext<'a> {
    pub cap_bytes: u64,                       // 窗口余量推导的本次上限（调用方算；恒 ≥ 提示句预算）
    pub stamp: Option<ClockStamp>,            // clock::StampGate 的产出；None＝不携
    pub net_notice: bool,                     // gate::egress 首次公网放行信号
    pub steer: Option<(String, String)>,      // (source, text)；上一边界消费到的 Steer
    pub offload: Option<OffloadSite<'a>>,     // None＝无 CAS 可用（纯截断退路）
}
pub struct Packaged { pub content: String, pub events: Vec<Payload> }   // events＝result_offloaded 载荷（入账归调用方）
pub fn package(result: &[u8], ctx: PackContext<'_>) -> Result<Packaged, AxError>;
```

- 定序：offload 恒先于截断；`len ≤ cap` →原样；`len > cap 且 len ≥ OFFLOAD_MIN_BYTES 且有 OffloadSite` → offload；否则纯截断（尾部留 `[truncated: N bytes]`，不入 CAS）。
- 信封三附件一处组装：正文后依序追加 clock 行／net_notice 行（恒一次：正在连接互联网提醒，英文定句）／steer 行（`user:`／`@ID:` 前缀）；三行字节不计入 cap（附件与负载分账，附件有自己的封顶常数在实现内断言）。
- 内容感知压缩分派表属 P3；本模块本期只持「原样／offload／截断」三臂，接口不预留分派参数。

### 8-8 runtime::offload（S3.09；形状 1；四不变量的独占定义处）

```rust
pub struct OffloadSite<'a> { pub cas: &'a mut memory::Cas, pub environment: &'a std::path::Path }
pub struct OffloadRecord { pub substitute: Vec<u8>, pub original: Locator, pub rest_path: std::path::PathBuf,
                           pub original_len: u64 }
pub fn offload(bytes: &[u8], cap_bytes: u64, site: &mut OffloadSite<'_>) -> Result<OffloadRecord, AxError>;
pub fn rematerialize(locator: &Locator, site: &mut OffloadSite<'_>) -> Result<std::path::PathBuf, AxError>;
```

- 四不变量逐条入断言：①先存后缩（入参恒为全量字节，cas.put 先于一切裁剪）；②替代体含提示句恒 ≤ 原件且 ≤ cap（提示句字节先扣）；③只有有损才存（调用者保证 len>cap 才进来；函数内再断言，违反＝E_INVALID_ARGS）；④替代体恒携 rest_path：物化只读文件于 environment，内容＝全量原件；命中既有 CAS 对象即直引（幂等）。
- 替代体形：头部字节＋`\n[offloaded: total N bytes; rest at <rest_path>; original <locator>]`；提示句 ASCII。
- rematerialize：rest_path 被外部清理后自 CAS 重建，字节一致（A7 第三断言）。

### 8-9 runtime::watchdog（S3.11；形状 1＋处置历史持有者）

```rust
pub struct Watchdog { /* corrections: u32、provider_failures: u32 —— 私有，逐 Run 一实例 */ }
#[non_exhaustive] pub enum Disposal { Proceed, CorrectiveSteer { text: String }, Freeze { reason: FreezeReason } }
#[non_exhaustive] pub enum FreezeReason { Stall, ProviderExhausted }
impl Watchdog {
    pub fn new() -> Watchdog;
    /// Consumes kernel::stall's verdict verbatim; never re-derives it.
    pub fn on_stall(&mut self, verdict: &StallVerdict) -> Disposal;      // 首次 Stall→CorrectiveSteer；再次→Freeze{Stall}
    pub fn on_provider_failure(&mut self) -> Disposal;                   // 前 WATCHDOG_PROVIDER_RETRIES 次→Proceed（重试归调用层）；超→Freeze{ProviderExhausted}
    pub fn fired_payload(&self, disposal: &Disposal) -> Result<Payload, AxError>;   // watchdog_fired 载荷（E_LOOP_SUSPECTED 的 carrier）
}
```

- 处置必分级：纠正 Steer 文本指名重复指纹；只有终局的处置被明拒。子 Run 监控：`Completion::Limit` 的呈现住 status.children（S3 类型已备，派生消费者 P2），本模块不重复存储子态。
- `WATCHDOG_PROVIDER_RETRIES=2` 为 pub(crate) 数据面（工程参数，改须本 SPEC 同集）。
- S3.11 落地记录：fired_payload 字段＝{action: steer|freeze, text|reason, corrections, provider_failures}；Proceed 拒绝成帐（无事不记）；纠正只发一次（corrections 计数），第二次 Stall 即冻——分级穷尽于 steer→freeze 两级，「停滞中间态」不另设（它就是 Stall verdict 本身）。

### 8-10 runtime::clock（S3.10；形状 1；纯格式化不采样）

```rust
// 关切时区的权威住 kernel::config::ClockZone（[clock] zones 属三层配置）：
// FrozenConfig 增 clock_zones: Vec<ClockZone>，freeze 增梯入参；本模块只消费不定义（一个权威）。
pub struct ZoneEntry { pub id: String, pub offset_min: i32, pub local: String }   // local＝"YYYY-MM-DD HH:MM"
pub struct ClockStamp { pub utc_ms: TimeMs, pub zones: Vec<ZoneEntry> }           // utc_ms 已按桶截断
impl ClockStamp { pub fn render(&self) -> String }   // 信封与人读共用的唯一文本形："clock: utc …; <id> …;"
pub fn stamp(now: TimeMs, zones: &[ClockZone]) -> Result<ClockStamp, AxError>;   // zones > CLOCK_ZONES_MAX → E_INVALID_ARGS；UTC 行恒首；空表即只报 UTC

pub struct StampGate { /* granularity、last_bucket: Option<u64> —— 私有；last_bucket 兼任首发标记 */ }
impl StampGate {
    pub fn new(granularity: ClockStampGranularity) -> StampGate;
    /// Emission rule: Off -> never; first result of the
    /// run -> once; Timestamped -> every result; Timeless -> only when the
    /// granularity bucket changed since the last emission.
    pub fn observe(&mut self, now: TimeMs, temporal: Temporal, zones: &[ClockZone])
        -> Result<Option<ClockStamp>, AxError>;
}
```

- 历法纯整数（civil-from-days，无 chrono 依赖；界证明携 `#[expect]`）；戳内容按 granularity 桶截断（同桶同字节，Timeless 去重因此有义）。A18 零字节：Off 时 observe 恒 None。

### 8-11 runtime::catalog（S3.10；形状 6＋渲染）

```rust
pub struct CatalogEntry { pub name: String, pub disclosure: String, pub expansion: String,
                          pub hash: Option<B3Hash> }   // V3.27：架上那份文档被读到时的哈希
pub struct SkillPin { pub name: String, pub hash: B3Hash }   // V3.27
pub struct Catalog { /* tools: BTreeMap<ToolName,…>、skills: BTreeMap、mode: Option<Mode> —— 私有 */ }
impl Catalog {
    pub fn new() -> Catalog;
    pub fn admit_tool(&mut self, meta: &ToolMeta) -> Result<(), AxError>;      // disclosure 非空；重名＝E_INVALID_ARGS
    pub fn admit_skill(&mut self, entry: CatalogEntry) -> Result<(), AxError>; // 只收阅览室准入者（准入求值归 city::policy，P1；本期调用方直供）
    pub fn set_mode(&mut self, mode: Mode);                                    // 只列本 Run 所处者
    pub fn render(&self) -> String;              // Resident 段的 catalog 部分：段头一行自述＋一行一件；BTreeMap 序恒定
    pub fn tool_defs(&self) -> Vec<ToolDef>;     // ChatRequest.tools 的唯一来源
    pub fn expand(&self, name: &str) -> Option<&str>;   // 第二级披露（怎么用）
    pub fn skill_pins(&self) -> Vec<SkillPin>;   // V3.27：本 Run 拿到了哪几份，当时各是什么字节
}
```

- **`hash` 是 `Option`，而那个 `None` 不是「没算」**：目录里另有两类条目的正文由本构建自己握着（mode 的纪律、dev 那一条），它们背后没有一份能在无人看着时改掉的文档。
- **pin 从 catalog 取，不重扫一遍书架**：catalog 已经是「本 Run 能够到什么」的权威，再扫一次就是在另一个时刻对同一个问题给第二个答案。

**P6.02：`render()` 与 `set_mode()` 接线**。它们自 S3.10 写下就**一个生产调用者都没有**，只有自己的测试在调。后果不是「少了一行文字」：工具走 `ChatRequest.tools` 到得了模型，而**阅览室准入的 SKILL 与本 Run 所处的 mode 从未到达任何模型**——`city::library` 的准入判定因此是一道没有下游的门。

接法：`Catalog::render()` 追在 `identity.segment_bytes()` 之后，合成 Resident 段。**不另开第五个槽**：一个居民能够伸手取到什么，与它是谁同属一类常住事实，且两者都随 Run 冻结，故前缀在整个 Run 的寿命里仍可缓存。装配层因此把 prefix 的组装移到目录建好之后。

**实测（一次真机派活）**：Resident 段 106 B → 1,176 B，差额 **1,070 B**（八件工具加一个 mode）。同一份派活下，模型被问「你被告知了哪些能力」时逐个点名 `archive, edit, exec, goal, plan, pr, signal, status`——其中 `plan` 就是 mode 的那一条，它在本卡之前从未被任何模型看见过。

**第二级披露今天仍不可达，原因写在这里而不是留给人撞**：SKILL 的 `expansion` 是 `city::holding_address()` 给的一个地址，坐在**保留前缀 `.sprawling/` 下**，而本构建的工具台里**没有读文件的工具**（`edit` 只改不读）。故 `render()` 故意不印那个地址：叫一个模型去读它取不到的东西，比不告诉它更坏。补齐它需要一件读工具，那是一项新能力而不是本卡的缺陷修复。

### 8-12 runtime::mode（S3.10；形状 6；P6.05 增 dev 入口）

```rust
pub const DEV_ENTRY: &str = "dev";
pub fn dev_entry() -> CatalogEntry;   // 一行披露，全部细则归 expansion
```

- **一个 Run 只被告知它所在的那个 mode**，于是没有任何 Agent 知道这座城自己的代码与 SPEC 是可改的。`dev` 行补上这一句，**而且只补一句**：三个模式的定义、阅读次序（SPEC → 代码 → 旁边的测试）与「下一步去跟人要模式」全在 expansion 里，由 `read` 按需取。**大多数会话不改这座城，就只付一行的价。**

### 8-12b runtime::mode 原有面（S3.10）

```rust
#[non_exhaustive] pub enum Mode { PlanGoal, Up, Sc, Ud, Experiment }
impl Mode { pub fn as_str(&self) -> &'static str;              // "plan_goal" | "up" | "sc" | "ud" | "experiment"
            pub fn catalog_entry(&self) -> CatalogEntry }      // 含 PlanGoal 退出条件四列
```

### 8-13 runtime::sandbox（S3.12；缝清单文件，形状 3＋4）

```rust
pub struct Fuel(pub u64);
pub struct Mount { pub host: std::path::PathBuf, pub guest: String, pub writable: bool }   // preopen＝mount scope
pub struct SandboxJob { pub wasm: std::path::PathBuf, pub argv: Vec<String>, pub env: Vec<(String, String)>,
                        pub stdin: Vec<u8>, pub mounts: Vec<Mount>, pub fuel: Fuel }
pub struct SandboxOutcome { pub stdout: Vec<u8>, pub stderr: Vec<u8>, pub exit: SandboxExit }
#[non_exhaustive] pub enum SandboxExit { Success, Failure { code: u64 }, FuelExhausted, Trap { message: String } }
pub trait Sandbox { fn run(&mut self, job: &SandboxJob) -> Result<SandboxOutcome, AxError>; }

pub struct WasmtimeSandbox;            // feature = "wasm"；wasip1 直跑（先按 preview1 落地）
pub struct EchoSandbox { /* 直通替身：stdout＝stdin 回声＋可注入脚本输出 */ }
pub struct FaultSandbox { /* 故障替身：逐次弹出预置 SandboxExit／fuel 耗尽／trap */ }
#[cfg(feature = "conformance")]
pub fn assert_sandbox_conformance<S: Sandbox>(sandbox: &mut S, job: &SandboxJob);  // 良序两连调不中毒＋outcome 形合法
```

- 能力面＝wasip1 preopen 集（Mount 逐条）；无网络能力（WASI p1 天然无 socket 宿主实现——Python 臂禁网的机械保证）；fuel 上限即 Fuel（耗尽＝FuelExhausted，不是 Err：宿主无故障）。
- 未授能力被拒的观察形：guest 内 open 失败→非零退出（Failure）；宿主恒不代 guest 隐藏失败。A10 三断言在真 wasmtime 上以手写 WAT 模块定形（不依赖 CPython 工件）；CPython-WASI 集成测试以环境变量指向工件（住机器本地的忽略目录，恒不入库），缺工件即 skip——`just check` 自足。
**A10 三断言结论书**（S3.12；证据＝`crates/runtime/tests/sandbox_a10.rs`，真 wasmtime 48.0.0，手写 WAT 不依赖任何外部工件）：

| 断言 | 观测形 | 结论 |
|---|---|---|
| fuel 内成功 | `fd_write` 写 `ok\n`，Fuel(1_000_000) | `Success`，stdout 逐字节相符 |
| 未授能力被拒 | 无 preopen 时 `path_open` 失败→guest 自行 `proc_exit(7)` | `Failure{code:7}`；**授予同一目录后同一 guest 转 `Success`**——此对拍使「被拒」是能力判定而非测试损坏 |
| fuel 耗尽中断 | 无限循环，Fuel(10_000) | `FuelExhausted`，且是 `Ok` 非 `Err`（宿主无故障） |

- **无出网的机械形需精确化**（S3.12 实测修正本 SPEC 原措辞）：wasip1 **确有** `sock_send`／`sock_recv`／`sock_accept`／`sock_shutdown` 宿主实现（对非 socket fd 恒返 `ENOTSOCK`）；它没有的是**获得** socket 的途径——无 `sock_open`／`sock_connect`／`sock_bind`。故导入 `sock_connect` 的 guest 直接链接失败（`E_SANDBOX_DENIED`），而 guest 能拿到的每一个 fd 都来自 preopen（均为目录）。这才是 Python 臂禁网的准确依据。
- **失败不得以默认值擦除**（rust-hardening Gate 5）：`try_into_inner()` 取不回管道、`get_fuel()` 报不出余量、退出码超 WASI 范围——三者均属**宿主故障**，恒返 `Err`；若以 `unwrap_or_default()`／`unwrap_or(1)` 兑成「空输出」「未耗尽」，就是把猜测冒充事实。
- **代价实测**：开 `wasm` feature 后 runtime 依赖面由 71 个 crate 增至 257 个（+186）；debug 下 `libwasmtime_wasi.rlib` 单件 186 MiB。故 feature 内藏是必要的，非可选修饰。
- 待办（不属本卡）：CPython-WASI 工件接入与体积实测回填性能册——工件住机器本地的忽略目录、恒不入库，缺工件即 skip，`just check` 自足。
- 门的缺口：`just clippy` 取 `--all-targets` 而无 `--all-features`，故 feature 内藏的代码逃过零警告门；本卡已手动跑 `clippy --features wasm,conformance --all-targets` 零发现。扩到 `--all-features` 要改 justfile，而 justfile 在 guard 辖区内。

- wasmtime 钉 48（2026-08 复核：含 GHSA-2r75-cxrj-cmph（path_open TRUNCATE 绕过，修于 44.0.2/45.0.0）与 CVE-2026-58494（hard-link/rename FilePerms 绕过，修于 45.0.3/46.0.1）两处修复——「钉版恒含权限绕过修复」的判据实例）。

**P4.02 增（sandbox）**：`AbsentSandbox` —— 未带执行引擎的构建在缝上的产品实现，逐次以 `E_TOOL_UNAVAILABLE` 拒并携替代臂。它存在的理由是**缺席要是一个判词而不是一个替身**：Echo 放在这个位置会对一个从未运行的 guest 回答「成功」，而第一个察觉的人是相信了那份输出的人。

### 8-14 runtime::tools 四件（S3.13 三件＋P6.04 read；形状 4；tools.rs 为纯索引）

```rust
// tools/exec.rs —— 三臂（ExecArm 住 kernel::tool）
pub struct ExecTool { /* workdir、mounts、python_wasm: Option<PathBuf>、sandbox: Box<dyn Sandbox>、
                        shell: Option<PathBuf>、fuel —— 私有；全由装配／执行器注入 */ }
impl ExecTool { pub fn new(…) -> ExecTool; }
impl Tool for ExecTool { /* meta：name=exec、effect=Write{domain}、temporal=Timestamped、render=Terminal */ }
// Program 臂：std::process::Command（workdir 钉定、环境变量白名单——secret 恒不透传）；本期唯一真子进程产地
// Python 臂：sandbox.run(python_wasm, argv=["python","-c",code], mounts)；组件缺失→E_TOOL_UNAVAILABLE＋alternative＝Program 臂
// Shell 臂：探测缺失即拒（E_TOOL_UNAVAILABLE，不是降级）；存在则 sh -c／cmd /C

// tools/edit.rs —— base_version 乐观并发＋写域双闸＋创建臂（整修卡 R1.02）
pub struct EditTool { /* city_root、writable: WriteDomain —— 私有 */ }
impl Tool for EditTool { /* meta：name=edit、effect=Write{domain}、render=Diff、temporal=Timeless */ }
// new(city_root, addr, writable: WriteDomain)：writable＝该 Run 的写域（rules.write_domain()）。
// 每次调用先判路径后碰盘：Address::parse 杀穿越（..／绝对路径／空段），WriteDomain::admits 杀域外与
// reserved prefix（E_OUTSIDE_WRITE_DOMAIN，recovery 报可写前缀清单）。此前只有工具静态声明过门，
// 模型选的 path 未经任何判定直接落盘——那是一个真漏洞，修在权威处而非 bench 里的第二份判定。
// args：{path, base_version, old, new}；version＝内容 B3Hash 前 16 hex；check_base 拒即 E_VERSION_CONFLICT；
// old 必唯一命中（零命中／多命中＝E_INVALID_ARGS 携计数）；回显＝unified diff＋new_version（逐次 diff 即回档粒度）

// tools/read.rs —— 一个参数，两条路（P6.04）
pub struct ReadTool { /* city_root、catalog: Rc<RefCell<Catalog>> —— 私有 */ }
impl ReadTool { pub fn new(city_root: &Path, catalog: Rc<RefCell<Catalog>>) -> Result<ReadTool, AxError>; }
impl Tool for ReadTool { /* meta：name=read、effect=Read、cost=Light、render=Generic、temporal=Timeless */ }
// args：{path}。先问 catalog，再当作地址。
// 创建臂：base_version=="new"（16 hex 永拼不出，无碰撞）→ 文件必不存在（存在＝E_VERSION_CONFLICT 报真实版本），
// old 必 ""，new＝全文；父目录自动建（域内已证）。缺文件而非创建形的拒词指向创建形；
// 缺参拒词报四字段契约。理由：没有创建能力的城里，Agent 在空房间里无法开始任何工作；
// 创建住 edit 而非新工具，因为「文件变更＋乐观并发」已是本工具拥有的唯一权威，“absent”只是版本的一个取值。

// tools/status.rs —— 十三字段（P3.06 追加第十三行）
pub struct StatusSnapshot { pub who: String, pub addr: Address, pub mode: Mode, pub ctx_used: Tokens,
    pub ctx_limit: Tokens, pub budget_usd: UsdMicros, pub budget_tokens: Tokens, pub trust: String,
    pub write_domain: String, pub locks: Vec<String>, pub worktree_path: String, pub worktree_disk: ByteLen,
    pub signals_pending: u32, pub children: Vec<ChildStatus>, pub now: Option<ClockStamp>,
    pub provider_mode: ProviderMode, pub neighbours: u32 }   // neighbours 在末尾，渲染序与声明序同一
#[non_exhaustive] pub enum ProviderMode { Normal, Degraded, LocalOnly }
pub struct ChildStatus { pub room: Address, pub kind: DelegateKind }   // P1.03：重塑
pub struct StatusTool { /* snapshot＋ children: Box<dyn Fn() -> Vec<ChildStatus>> */ }
impl StatusTool { pub fn watching(snapshot: StatusSnapshot, children: Box<dyn Fn() -> Vec<ChildStatus>>) -> Result<StatusTool, AxError>; }
impl Tool for StatusTool { /* meta：name=status、effect=Read、temporal=Timestamped、render=Generic */ }

// ToolBench 住 turn.rs（S3.13 同卡加入）：按 Effect 过门是回合层职责（Handoff 裁定 10），不另立 bench 模块。

- **`children` 为何重塑（P1.03）**：旧形状 `{run, phase, ctx_used, ctx_lock}` 预设子已在跑。真实情形是子 Run 在父嚽结之后才开，故父自己那一跑里 **子既无 run id 也无上下文读数**——四个字段里三个只能填零，而零与未知是两件事。现形状只携得出口的两件：派到哪个房间、哪一类代理。
- **`neighbours` 追加在末尾而不插入到 `signals_pending` 旁边（P3.06）**：冻结序存在的理由是字段表增长时居民的习惯仍可迁移，而一次插入会把前十二行里的一半挪位。它只报**人数**不报名单：名单长度随人口增长，而 `status` 是一份定长文本（`render_children` 已为同一条理由被压成一行）；详情归 `neighbours` 工具，city-SPEC §8-15b。
- **数的是人，不是地址**：一间没人站着的房间没有读者，把它计入会让 `neighbours: 3` 读起来像「有三个人可以说话」而实际上一个都没有。空房间仍然在工具的答案里，因为它对 delegate 与搬入是真信息。
- **`children` 是闭包而不是快照字段**：派活发生在 `status` 工具造好之后，一份开跑前拍的快照永远是空的。派生台住 `collab`，而 depmap 不允许 runtime 依赖 collab，故本模块只收一个答「现在派了哪些」的闭包，装配层把台接上去——与 `RunHooks` 四个闭包同一纪律：第二实现不存在时不引 trait。
// runtime::compaction（P3.14；形状 6 数据面＋形状 1 判定）
pub enum Content { Prose, Code, Diff, Log, Structured, Table, Unknown }   // 七类，Unknown 是其中之一
pub enum Strategy { Keep, Head, Ends, Tail, Offload }
pub fn detect(text: &str) -> Content;                       // 前几行上的前缀与计数，顺序即设计
pub fn plan(content: Content, size: ByteLen, budget: ByteLen) -> Strategy;
pub fn compact(text: &str, budget: ByteLen) -> (String, bool);
// 硬不变量：结果恒不大于输入，且在出口再验一次（真长了就退回原文）。切口落在字符边界。
// Structured 与 Unknown 恒不截断：被截断的 JSON 比缺席的 JSON 更糟；未知内容不拿猜测去丢东西。
// 机制面（把大结果移出窗口）仍归 offload——本模块只答「缩不缩、留哪一头」。

// runtime::mode 的准入面（P3.08；形状 1 判定）
pub struct Produced { pub tests_passed: Option<bool>, pub contract_moved: bool,
                      pub held_in: Option<bool>, pub held_out: Option<bool> }
pub enum Admission { Lands, Refused { because: &'static str, alternative: &'static str } }
pub fn admits(mode: Mode, produced: &Produced) -> Admission;
// `None` 不是 `Some(false)`：「没测」与「测了没过」是两件事。证据以 bool 入参而非 eval 的类型，
// 因为 eval 在本 crate 之外，而这里问的不是证据怎么来的，是够不够。UD 是唯一要双验证的模式。

// runtime::redact（P3.04；形状 1 判定）——入口只有一个：`Turn::call` 写 model_returned 之前。
// 窗口块已在此前取出，故思考块签名不受影响；历史与上下文是两个汇，只有一个是永久的。
// 替换物是 `secret:redacted/<b3-16>` 标记而非 Vault 条目：模型复述的钥匙不是城被托付保管的凭证，
// 存它等于给它一条没人要求过的命，而哈希前十六位已足以看出两处是否同一个值。
pub fn redact(payload: &Map<String, Value>) -> (Map<String, Value>, u32);
pub fn redact_text(text: &str) -> (String, u32);

pub struct ToolBench { /* tools: BTreeMap<String, Box<dyn Tool>>、domain: WriteDomain、registry: Registry、
                          taint: TaintSet、seen: BTreeSet<IdemKey>、prior_public_egress: bool —— 私有 */ }
impl ToolBench {
    pub fn new(domain: WriteDomain, registry: Registry) -> ToolBench;
    pub fn register(&mut self, tool: Box<dyn Tool>) -> Result<(), AxError>;
    /// Gate routing by declared Effect（Handoff 裁定 10：收进回合层，executor 归还薄形）：
    /// exec 先 forecast（Suspected → **强制 checkpoint 先行**，见下）；Write → domain 门；
    /// Egress → egress 门（target 由调用自报 host，不自报即 E_INVALID_ARGS）；Spend → 门已接但 P1 前无实例；
    /// Spawn → delegation 门（恒 Escalate；granted 命中即放行，未命中即 Pending）；
    /// Govern → govern 门（同形；提案正文由调用的 `text` 参数取，进 action_desc 供人过目）；
    /// Deny → 以 refusal 作 tool_result 回流（不吞掉回合）；Escalate → BenchOutcome::Pending 回流（S3 无应答面）。
    pub fn invoke(&mut self, call: &ToolCall, key: &IdemKey, ctx: &GateContext)
        -> Result<BenchOutcome, AxError>;
    /// R2.20b：上面那句「按 Effect 过门」自己的名字。`None` 即此门已开，
    /// 工具可跑；`Some` 即门已代这次调用给出答案，工具不跑。私有，公开面不变。
    fn admit(&mut self, call: &ToolCall, name: &str, effect: &Effect, ctx: &GateContext)
        -> Result<Option<BenchOutcome>, AxError>;
    /// 一扇门的判定对本 bench 意味着什么。三处 Escalate 的「人已经允过的
    /// cluster 不再问第二遍」原本各写一遍，现在只住这里。
    fn settled(&self, outcome: GateOutcome) -> Option<BenchOutcome>;
    /// 同形：两处出网判定的「首次公开出网要记下来」只住这里。
    fn crossed(&mut self, outcome: EgressOutcome) -> Option<BenchOutcome>;
    /// S3.14 长入：包信封的调用者要读 `temporal` 才知道时钟行该不该发。
    pub fn meta_of(&self, name: &str) -> Option<&ToolMeta>;
    pub fn with_checkpoint(self, checkpoint: Checkpoint, scope: &str) -> ToolBench;
    /// P1.04：本 bench 服务的那份活。Spawn 门要铸一个人答得出的条目，
    /// 条目要有 actor（问谁）与 artifact（看什么）；两者都不在一次工具调用里。
    /// 未给即拒（fail-closed）——一个人问不到的派生就是没人批准的派生。
    pub fn for_job(self, asking: Address, job: Locator) -> ToolBench;
    /// P3.03: a cluster the person already allowed. An escalation whose
    /// cluster is granted runs instead of parking, which is what lets an
    /// answer carry the work on rather than send it back to the door it
    /// was just let through. Granted per **cluster**, because that is the
    /// unit the person was shown and answered in.
    ///
    /// The caller folds these from `approval_resolved`; the bench does
    /// not read history, because it runs inside a drive that owns the
    /// ledger and a second reader of that would be a second answer.
    pub fn grant(&mut self, cluster: ClusterKey);
}
```

- L0 三件恒列 prefix（City.md 只放这一级）；catalog 只收 L2——L0 不进 catalog（名字即文档）但 tool_defs 恒含三件（wire 面要 schema）。
- **S3.13 语义修正（红转绿抓出）**：本 SPEC 期初写的「Suspected → Discard 门」与 kernel 既有设计冲突，以 kernel 为准。理由：`DiscardRequest` 只有 `Planned`／`Unplanned` 两变体，而 `decide` 对 `Unplanned` **恒判 Deny(NoRestoration)**——把 forecast 的预判包成 Unplanned 送进门，等于让任何含 `rm ` 的 exec 调用全被拒。`kernel::discard` 的注释早已写明正确意图：「text prediction is obfuscatable by design — hits route conservatively, and the git checkpoint net (S3) is the honest backstop」。故 **Suspected 不拒而围栏**：强制 `checkpoint.wave_pre` 先行再放行，删掉的东西因而可回档；**无 checkpoint 网时才拒**（`E_TOOL_UNAVAILABLE`），因为「无保护地跑」是唯一没人选择的结局。此路由使 A14 的先行半链在 exec 臂上机械成立。
- ToolBench 持 `Option<Checkpoint>` 具体类型而非新 trait：checkpoint 只有一个实现，为尚不存在的第二实现引缝会造空抽象（AGENTS.md：trait 只在已有第二实现的缝上引入）。
- **`BenchOutcome` 去掉 `#[non_exhaustive]`（整修卡 R2.16）**：它是判定输出，而本文对 `PhaseOutcome` 写的规则已经辖到它——「14.3 的 non_exhaustive 规则辖 wire 冻结枚举，不辖判定输出」。全工作区扫一遍：`kernel` 的八个判定枚举无一标它（`budget.rs` 行内写着「Deliberately exhaustive verdict enum」），**`BenchOutcome` 是唯一的例外**，于是两个下游各背着一条永不执行的 `_ =>`——而那正是 §7「新增一种答案而不回答它就不编译」要护的东西。收口是编译红：摘掉属性即得两条 `unreachable pattern`（citysim 一条、sprawling 一条），`-D warnings` 下即错，删掉它们才绿。下游从此必须穷尽匹配四臂。
- **三条规则各回到一处（整修卡 R2.20b）**：`invoke` 曾为 246 行，是 `length` 门报出的两个对象之一。拆它时量到三处重复：① `GateOutcome::Escalate` 的「granted 命中即放行」写了**三遍**（Write／Spawn／Govern）；② `EgressOutcome` 的「首次公开出网要记下来」写了**两遍**（Connector／Egress）；③ `serde_json::to_vec` 扫描参数写了两遍。三条都是规则而不是巧合：一份人给过的允许在三个地方各有一份实现，就是三个可以各自漂走的权威。归位后：`settled` 一处、`crossed` 一处，`admit` 成为那个 `match &effect` 自己的名字。尺寸 246 → 65（`admit` 139、`settled` 11、`crossed` 13）。行为逐字不变，公开面不变。
- `BenchOutcome` 四态：`Ran{outcome, fenced}`（fenced 携围栏 oid，供波后补记）／`Refused{refusal}`（回流不终止回合）／`Pending{item}`／`Duplicate`。dedup 先于任何副作用；**key 在过门之后才记入 seen**，故被门拒的调用重试不算重放。
- status 的 result 是**按冻结序渲染的文本**而非 JSON 对象（S3.13 红转绿抓出）：`serde_json::Map` 对键排序，JSON 对象没有读者可依赖的序，「冻结序」会悄悄变成字母序。序是「模型读到的东西」的属性，故落在模型读到的地方。
- 注意本期无 Egress／Spend 工具实例（出网代理 P1）：两门路由代码落地以测试替身驱动，接线台账仍记「计划内待接」至真实例出现。

### 8-15 runtime::run（P1.01；形状 5 typestate 机；**run 事件序的唯一权威**）

```rust
pub struct Run<S> { /* plan、window、turns、last_turn_t —— 全私有 */ }
pub struct Active(/* 私有 */);   pub struct Frozen { /* completion、turns */ }

pub struct RunPlan {                 // 一个 Run 的全部常量，调用方先备齐
    pub run: RunId, pub who: String, pub addr: Address,
    pub task: String, pub goal: String, pub job: Locator,
    pub parent: Option<RunId>,                            // P1.03：派活给它的那个 Run
    pub budget_turns: u32, pub budget: BudgetCap,         // R2.11：回合上限，以及花销天花板
    pub shape: CallShape,
    pub prefix: FrozenPrefix, pub policy: BuildingPolicy, pub tools: Vec<ToolDef>,
    pub skills: Vec<SkillPin>,                            // V3.27：阅览室准进了什么，当时各是什么字节
}
```

- **`skills` 写进 `run_started` 载荷，且无条件写**（空则空数组），理由与 budget 两栏同一条：一个时有时无的 key 是一个读者得猜的形状，而「这栋楼一个都没准进」本身就是一件值得记下的事。进账本而不只留在进程里，是因为「它变了没有」需要一个**早一次的读取**，而进程一走就只剩账本说得出这一轮到底拿到了哪些字节。

#[non_exhaustive] pub enum SafePoint { BeforeAssemble{turn:u32}, BeforeCall{turn:u32}, BeforeWave{turn:u32}, BeforeSpawn{turn:u32} }
pub enum Advance { Turned, Concluded(Completion) }        // 穷尽；新结局逼每个调用方表态

pub struct RunHooks<'a> {            // 四个闭包，不是四个 trait：本模块只有一个消费者形式
    pub now: &'a mut dyn FnMut() -> Result<TimeMs, AxError>,        // 时间入参，本模块恒不采样
    pub interrupt: &'a mut dyn FnMut(SafePoint) -> Interrupt,       // 安全点由我定，信号由你答
    pub fence: Option<&'a mut dyn FnMut(TimeMs) -> Result<Payload, AxError>>,  // 波前 checkpoint
    pub invoke: &'a mut dyn FnMut(&ToolCall, TimeMs) -> Result<ToolOutcome, AxError>,  // 回合时间戳随行
}

impl Run<Active> {
    pub fn dispatch(plan: RunPlan, ledger: &mut dyn Ledger, hooks: &mut RunHooks<'_>) -> Result<Run<Active>, AxError>;
    pub fn advance(&mut self, ledger: &mut dyn Ledger, model: &mut dyn Model, hooks: &mut RunHooks<'_>) -> Result<Advance, AxError>;
    pub fn freeze(self, ledger: &mut dyn Ledger, handoff: &Handoff, completion: Completion, hooks: &mut RunHooks<'_>) -> Result<Run<Frozen>, AxError>;
}
impl Run<Frozen> { pub fn completion(&self) -> &Completion; pub fn turns(&self) -> u32; }

pub fn drive(plan: RunPlan, ledger: &mut dyn Ledger, model: &mut dyn Model,
             hooks: &mut RunHooks<'_>, handoff: &Handoff) -> Result<Run<Frozen>, AxError>;
```

- **为什么要这个模块**：「Dispatch → N 回合 → 冻结」的事件序先前只存在于 `citysim::executor`。真城再写一遍就是两个权威，而两者一旦漂开，**仿真继续绿而真城错**——仿真的全部价值恰好建立在它跑的是同一份代码上。故 citysim 改为本模块的调用方，23 剧本从此直接验证生产回路。
- **`run_started.parent`**（P1.03）：只在派生开的 Run 上出现。父子关系先前只存在于「两行相邻」这个巧合里，而相邻不是一个可查询的事实；写进载荷之后，前端折得出树，离线重放也折得出同一棵树。
- **`run_started.usd_micros` 与 `run_started.tokens`**（整修卡 R2.11）：一跑被派出去时的花销天花板。写它与写 `parent` 同理——**一个进程死后，「这跑当时允许花多少」只剩账本能回答**。先前它只活在装配层的一个局部变量里，于是一跑因待批而停下、被批准后续上的那一跑天花板归零（sprawling-SPEC §8-23 查出、§8-25 修复）。两个键是 `u64` 整数，符合确定性第六条；`fixtures/golden-p0` 随本卡重生（`GOLDEN_WRITE=1`），这是它存在的用法。
- **时间纪律**：dispatch 采两次（checkpoint、run_started），每回合一次；**自然结束与预算耗尽时 freeze 再采一次**，handoff 用它、run_frozen 用它＋1（两行同一件事，不值两次采样）；**取消时 freeze 沿用被打断那个回合的时间戳**，因为这次冻结属于那个回合而不是一件新事。三条合起来使一个计数器闭包（citysim）与一个壁钟闭包（真城）在同一驱动下各自正确。
- **结束判定**：`calls_made == 0` 即 `Completion::Done(Evidence[model_returned])`；跑满 `budget_turns` 即 `Completion::Limit`；任一安全点命中 Cancel 即 `Completion::Cancelled`。三条均经 `freeze` 出口，故 **handoff_written＋run_frozen 是唯一出口**，无第二条退路。第四点 `BeforeSpawn` 与前三点同权：命中即 `Cancelled`，那个回合的 assistant 与 tool results **不入窗**，因为窗口前推是「回合成立」的后果而不是它的一部分。
- **Window 归驱动持有**：入窗内容就是回合报告的前推结果（assistant＋tool results），放在调用方手里等于把一条不变量交给每个调用方自己维护。
- **四个闭包而非四个 trait**：第二实现尚不存在，而本库的纪律是 trait 只在已有第二实现的缝上引入（同 8-3 的 invoke 闭包）。`RunHooks` 自身只是四个引用的容器，不持策略。
- **字节不变是本卡的验收判据**：同一堆剧本、同一份 `fixtures/golden-p0`，换了驱动实现而字节不动——这才能证明“提取”是提取而不是重写。

### 8-16 runtime::digest（P1.07；形状 1 判定＋形状 2 值类型）

```rust
pub struct StructureNode { pub level: u8, pub title: String, pub offset: ByteLen, pub span: ByteLen }
pub struct Digest { /* source、origin、structure、prose —— 私有 */ }
impl Digest {
    pub fn structural(source: B3Hash, origin: Option<Locator>, text: &str) -> Digest;
    pub fn with_prose(self, prose: String) -> Digest;      // 消费并返回：带 prose 的是另一个值
    pub fn is_suspect(&self) -> bool;  pub fn window_header(&self) -> String;
}
pub fn structure_of(text: &str) -> Vec<StructureNode>;      // 纯、全函数

pub struct Breaker { /* limit、consecutive */ }
pub enum BreakerVerdict { Attempt, Open { after: u32 } }
pub enum DigestOutcome { Cached(Digest), Fresh(Digest), Structural { digest: Digest, reason: AxError } }
pub fn digest_once(text, origin, breaker, cached: &mut dyn FnMut(&B3Hash) -> Result<Option<Digest>, AxError>,
                   write_prose: &mut dyn FnMut(&str) -> Result<String, AxError>) -> Result<DigestOutcome, AxError>;
```

- **结构是读出来的，prose 是写出来的**：前者机械可复现，与原文恒不冲突；后者恒带 `suspect`，**没有清除该标记的方法**——摘要不会因为被读两遍就不再是摘要。
- **`window_header` 把可疑说在读者看得见的地方**，并给出原文位置：与原文冲突时以原文为准，这条要能被执行而不只是被相信。
- **一个内容哈希一生只摘一次**：顺序即全部策略——先哈希、再问缓存、再读结构、最后才花一次模型调用；熔断打开时连那一次也省掉。
- **熔断按次数不按时间**：判定路径里放墙钟会毁掉重放，而「连续三次失败」是调用方可复现的事实。一次成功即复位：间歇性的 provider 与坏掉的 provider 是两种情况。
- **失败不升级为错误**：`Structural{digest, reason}` 仍带完整结构树——摘要失败的文档仍然是一份有标题的文档。
- **模型调用归调用方**：本模块决定问什么、信什么，`bin::assembly` 持有 provider（同 `runtime::run` 的钩子形状）。

### 8-17 runtime::diagnostics（P1.13；形状 4 薄壳＋形状 6 数据面）

设计权威是 `docs/logging.md`；本节只记接口与三处口径差异。

```rust
#[non_exhaustive] pub enum Level { Refuse, Effect, Decide, Trace, Wire }  // 全序：层底控到该级为止
impl Level { pub const DEFAULT: Level = Effect; pub const ALL: [Level; 5]; pub fn parse(&str) -> Option<Level>; }
pub struct Site<'a> { pub run: RunId, pub seq: Seq, pub module: &'a str }   // 三字段必填
pub type Sink = Box<dyn FnMut(&str) + Send>;
pub struct Diagnostics { /* floor: Option<Level>、sink —— 私有 */ }
impl Diagnostics {
    pub fn new(floor: Level, sink: Sink) -> Diagnostics;
    pub fn off() -> Diagnostics;
    pub fn floor(&self) -> Option<Level>;
    pub fn admits(&self, level: Level) -> bool;
    pub fn write(&mut self, level: Level, site: Site<'_>, message: &str);
    // 无读方法。这是本模块全部保证的形状半边
}
pub fn redact(&str) -> String;   pub const REDACTED: &str = "secret:redacted";
```

- **无读方法即全部形状保证**：「判定与恢复逻辑不读日志」不靠纪律，靠这一点——把一行读回来在类型上拼不出。推论就是收口条件：删光日志，行为、重放与总账逐字节不变。
- **行上恒无时间戳**（与 `docs/logging.md` 早期口径的差异，已回写该文）：锦点是 `seq`——两条时间线靠一个整数对齐，而采样壁钟会在一个不允许采样的库里开第二个时间源。想要时间的 sink 在装配层自己加。
- **坐标由 Ledger 自己说**：`memory::JsonlLedger::position()`（本卡新增，返回「现在写一条会落在哪」）。只给位置不给内容：一个能读记录的访问器会把判定逻辑引到它正在写的账上去。
- **双重防线**：`Sealed` 无 Debug/Display，入行在类型层就不成立（反例 `tests/ui/log_a_credential.rs`）；普通字符串里的明文由 `kernel::scan`——**同一个**扫描器，不是第二个——就地换成 `secret:redacted`。不丢整行：周围那句话通常正是读者要的。
- **不引 `tracing`**：它在此处的唯一功能是跨 `await` 携模块名的 span，而回合路径是同步的，该功能今天无消费者。理由已回写 `docs/logging.md` §7。
- **写入方三处**（§6 的三类各一）：命令被拒（`refuse`，写在 `handle` 而非调用方，因为每个调用方都要）；endpoint 附着与探测结果（`effect`）；dispatch 跑完（`effect`，作为指向 Ledger 的指针）。

## 8.5 两个设计

**第二对（S2，turn 侧）**：中断作相变入参（选中）vs 独立 `cancel()` 方法。后者表面更直观，但 cancel 方法可在任意持有点被调＝相内中断可表示，A9 退化成时序约定；选中方案把边界快照做成相变函数的形参，相内无入口，结构即断言。代价：调用方每相必须显式给 Interrupt（哪怕 None）——这个啰嗦是刎意的：它迫使执行器在每个边界问一次信号面。

**首对（S1）：fork 消费 VerifiedLedger**——分叉前必先验链，类型上把「从未验证的序列分叉」做成不可表示；分叉正确性与重放正确性因此是同一条断言。
**B（落选）：fork 直接吃原始行**（`prefix(lines: &[Vec<u8>], at_seq)`）——少一次验证成本，但打开「对损坏历史分叉」的路径，且 at_seq↔行号对应要自行重解 envelope＝第二解析权威。落选理由：验证成本 O(n) 在分叉频率下可忽略，而不变量 14（citysim 检查器）需要的正是 A 的类型保证。另 `verify_dir` 命名族落选：与 `verify_ledger_dir` 二选一，取后者（dir 一词泛滥易撞 S3 worktree 面）。

## 9 工作流程

`just replay <log>`（S1.11 接线 bin）→ `verify_ledger_dir` → 全绿报 tail_seq／行数，违规报 three-part。citysim 检查器与 A19 测试直接调 `verify_lines`／`prefix`。

## 10 实现逻辑

envelope 探查与全解共用 kernel 的解析（Value 探查仅取五键，不建第二记录类型）；行号从 1 计（人读）；错误 recovery 字段给「重放同一夹具于更新版本」或「检查介质」两句可执行建议。

## 11 边界枚举

空序列（合法：VerifiedLedger 空，tail_seq=None；fork 于其上恒越界）；**目录存在但不含任何账本段**（在本模块合法且与空账本同形；人输入路径的拒绝在 CLI）；单行创世；`at_seq=FIRST`（前缀＝仅创世行）；`at_seq=tail_seq`（前缀＝全量）；ig:true 且 kind 已知（照常全解，ig 只授未知时的跳过权）；篡改中段一字节（链断于下一行报错）；两段夹具跨段验证（memory 读面已拼平）。

## 12 错误处理（逐码答「能否定义掉」）

- `E_INVALID_ARGS`（at_seq 越界）：不可定义掉——「从已冻结 Run 最后事件之后分叉」是用户可达输入（§19.1 点名）；静默夹取是被明拒的替代。
- `E_LOG_VERSION_UNSUPPORTED`（v 判向＋未知 kind 无 ig）：不可定义掉——数据比二进制长寿。
- 链断/seq 洞/非规范字节：以 `E_CAS_CORRUPT` 报（存储完整性族；subject=行号与路径）——能否定义掉＝「介质位腐烂在设计边界外」，同 memory-SPEC §12。

## 13 依赖选型

kernel、memory（读面＋S3 增 cas 消费）；serde_json（envelope 探查）。dev：proptest、tempfile、trybuild、insta（S3 增：prefix golden）。
S3 增：`wasmtime = "48"`（feature `wasm` 内藏，钉版理由见 §8-13；wat 为 dev 依赖供 A10 模块）；`similar`？否——unified diff 自写最小形（edit 回显只需逐行对照，不引第三方 diff 库；被否理由：依赖面换一处 80 行纯函数，不值）。其余无新第三方（分段哈希经 kernel `B3Hash::digest`，不直依 blake3）。

## 14 硬编码声明

无（行号计法与 recovery 文句不构成行为常量）。

S3 增三处 pub(crate) 数据面（改须本 SPEC 同集）：`WATCHDOG_PROVIDER_RETRIES=2`（§8-9）；信封附件封顶 `ENVELOPE_ATTACH_MAX_BYTES=1024`（§8-7：附件与负载分账的断言界）；net_notice／truncation／offload 提示句三定句（ASCII，住 pipeline／offload 实现内，改句＝改入窗字节＝过本 SPEC）。

## 15 影响面

citysim 链检查器（S1.11）复用 verify_lines；bin `replay` 子命令接线（S1.11）；S2 prefix 重建器将消费 VerifiedLedger——接口本期定形，只加不改。
S3：assemble 签名长入波及 citysim 执行器（同集更新）；kernel::model 增 canonical 类型波及 ScriptModel；ToolBench 收走 executor 门闭包（归还薄形）；E_TOOL_OUTCOME_UNKNOWN 补写面（replay 新增 dangling 检测）供 resume 路径消费。

## 16 测试与约束

单测：五步各拒绝分支＋ig 跳过；fork 越界；fork_draft 载荷形。proptest：对任意合法 draft 序列（经内存 Ledger 物化）verify 恒过；任意单字节翻转恒拒。A2/A19 演示测试入 crates/runtime/tests/。约束：clippy 零告警。
S3 增：A4 golden（build_prefix 重跑逐字节同）；A15（rebuild_prefix 对拍）；A7 往返四断言；A18 零字节；watchdog 分级序；A10 三断言（feature `wasm` 下真 wasmtime＋WAT）；L0×失败注入矩阵（三臂×（正常／工具错／拒收））；ToolBench 门路由（Deny 回流／Escalate 回流／dedup 先于副作用）。

## 17 模型体验

零字节：replay/fork 是离线设施；其产物（分叉 Run 的入窗历史）经 S2 prefix 组装间接入窗，本模块自身不产生任何 prefix 字节。

## 18 文档同步

ARCHITECTURE §6 runtime 表逐卡状态翻转（turn/prefix/handoff 随 S2.01/S2.02；S3 九模块逐卡）；接线台账同 PR 登记；S3 完备化的「只加不改」取义见 §8-6（三不变量不动，相变入参按语义长入，消费者同集）；kernel-SPEC §8-23/§8-24 随 S3.01/S3.13 同集改；api-baseline 随每张改公开面的卡重算。

### RunHooks 多一个：说到一半的话往哪去（V3.13）

```rust
pub struct RunHooks<'a> {
    pub now: ...,
    pub interrupt: ...,
    pub fence: ...,
    pub invoke: ...,
    pub deltas: Option<&'a mut (dyn FnMut(&str) + 'a)>,
}
```

**`None` 是承重的，不是缺省值。** 一个增量改变不了 run 的任何判断，所以「没人看」的驱动器就不向 provider 要流：`Turn::call` 在 `None` 时走 `Model::call`，字节与从前一模一样。citysim 与离线重放因此一字未改——**这是这条改动不碰确定性的全部理由**。

**它不返回 `Result`。** 增量不是判断：下游任何东西都不得据它分支，而一个能拒绝的 sink 会让一个显示细节有能力弄失败一次调用。

**写进账本的那句话只从 `ModelReturn` 来。** `model_returned` 的载荷此前怎么写，现在还怎么写——增量恒不参与拼装它。于是「页面看到的」与「账本保存的」不可能出自对同一个回复的两次读法；流被切断表现为读取错误，永不表现为一个变短的回答。

**`Turn::call` 的 `'sink` 是显式命名的。** 调用方（`drive`）持有 sink 跨越整个 run 并把它交给每一轮；生命周期省略时，重借需要收缩 trait object 自己的生命周期，而 `&mut` 不允许。这不是风格，是这个签名必须显式的原因。
