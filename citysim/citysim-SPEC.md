# citysim-SPEC.md

> 工作区成员：`citysim`（dev-only，第二个 Main，不占产品拓扑）。本 SPEC 先于代码存在。
> Stage 1 两件（内存 Ledger＋链检查器）＋Stage 2 三件（剧本模型／脚本工具／薄执行器，§8-2）＋Stage 3 真适配器换入（§8-3）。

## 1 需求拆解

| 卡 | 件 | 一句话 |
|---|---|---|
| S1.11 | `mem_ledger` | kernel Ledger 的第二适配器：全内存、确定性、conformance 的对照实现 |
| S1.11 | `checker` | 不变量检查器首条：链完整且 seq 连续 |
| S2.03 | `script_model` | kernel Model 的第二适配器：脚本驱动的 ModelReturn 序列，确定性 |
| S2.03 | `script_tools` | kernel Tool 的第二适配器：脚本工具＋按名分发闭包，全失败模式可注入 |
| S2.03 | `executor` | 薄执行器：Dispatch→四相回合×N→run_frozen；取消注入点＝A9 先行 |

## 2 验收标准

- `MemLedger` 过 kernel conformance 六断言（与 JsonlLedger 同一套——V3 兑现「缝」）。
- 同一 draft 序列灌 MemLedger 与 JsonlLedger，raw 行逐字节相同（规范字节住 kernel 的实证）。
- checker 对合法序列静默通过；对篡改序列报出首个断点行。
- 跨 OS 字节夹具：`fixtures/golden-s1/` 的脚本化序列在本机重建后与夹具逐字节相同（CI 三平台恒跑同一断言）。

## 3 假设与歧义

citysim 不受 ARCHITECTURE §6 模块表约束（表只辖 crates/**），但 MPL 头、lexicon、lints 全库同规。S2 起 `just sim` 的入口仍是本 crate 测试（固定剧本＝测试用例）；种子驱动的随机剧本批随 S4 故障面落地。Dispatch 的「先落 JOB.md 再产事件」在 sim 里以 `checkpoint_committed`（确定性假 oid＝B3Hash 派生前 20 字节 hex）代文件面——模拟适配器的职责即伪造外部世界，事件序与真城同形。

## 4 现状分析

空壳 lib。无性能议题。

## 5 权威信源

citysim 定位、四件模拟适配器、检查器十五条；kernel-SPEC §8-9；runtime-SPEC §8-1。

## 6 命名统一

MemLedger、checker、invariant（编号取检查器清单）。

## 7 模块边界

```
mem_ledger ──▶ kernel（Ledger trait＋event＋conformance feature）
checker    ──▶ runtime::replay（verify_lines 复用，不建第二验证权威）
```

**不做什么**：不落盘；不采时钟（t 由剧本注入）；不实现除 Ledger 外的三件模拟适配器（剧本模型/脚本工具属 S2.03，FaultFs 已住 memory）。

## 8 接口先行

```rust
pub struct MemLedger { /* lines: Vec<Vec<u8>>, next_seq, prev */ }
impl MemLedger { pub fn new() -> Self;  pub fn raw_lines(&self) -> &[Vec<u8>]; }
impl kernel::Ledger for MemLedger { … }
impl kernel::conformance::LedgerInspect for MemLedger { … }   // citysim 恒开 conformance feature

/// Invariant 1: chain intact, seq contiguous.
pub fn check_chain(lines: Vec<Vec<u8>>) -> Result<(), AxError>;   // replay::verify_lines 薄封
```

### 8-2 S2.03：剧本适配器与薄执行器

```rust
pub struct ScriptModel { /* VecDeque<ModelReturn> */ }
impl ScriptModel { pub fn new(script: Vec<ModelReturn>) -> Self; }
impl kernel::Model for ScriptModel { /* 逐次弹出；耗尽后恒回空 calls（自然收束） */ }

pub struct ScriptTool { /* meta、outcomes: VecDeque<Result<ToolOutcome, AxError>> */ }   // impl kernel::Tool
pub struct ScriptToolSet { /* BTreeMap<String, ScriptTool> */ }
impl ScriptToolSet { pub fn new(tools: Vec<ScriptTool>) -> Self;  pub fn empty() -> Self;
                     pub fn invoke(&mut self, call: &ToolCall) -> Result<ToolOutcome, AxError>; }
                     // 执行器以 `&mut |c| tools.invoke(c)` 适配 turn 的闭包形参；不另造 invoker 工厂
                     // 未知工具名 → E_TOOL_UNKNOWN＋nearby＝已注册名；耕尽脚本 → E_TOOL_UNAVAILABLE

pub enum CancelPoint { BeforeAssemble { turn: u32 }, BeforeCall { turn: u32 }, BeforeWave { turn: u32 } }
pub struct Scenario { pub run: RunId, pub who: String, pub addr: Address, pub task: String,
                      pub goal: String, pub job_md: String, pub model: ScriptModel,
                      pub tools: ScriptToolSet, pub cancel: Option<CancelPoint>, pub budget_turns: u32 }
                      // run/who 由剧本注入（citysim 禁随机）；job_md 是 JOB.md 内容，假 oid 由其哈希派生
pub struct ScenarioReport { pub lines: Vec<Vec<u8>>, pub completion: &'static str /* done|cancelled|limit */ }
/// The thin executor (second Main): Dispatch → N four-phase turns → freeze.
/// Deterministic: t 单调递增每步 +1ms，无时钟采样；无随机。
pub fn run_scenario(scenario: Scenario) -> Result<ScenarioReport, AxError>;
```

- 事件序（无取消正常收束）：`checkpoint_committed`（JOB.md 先落）→ `run_started` → 每回合 `prompt_assembled→model_called→model_returned[→tool_called→tool_result]*` → 空 calls 回合后 `handoff_written` → `run_frozen{completion:done, evidence:[末 model_returned]}`。
- 取消在指定边界注入 `Interrupt::Cancel`：事件序断言＝cancel_received 后无新 model_called/tool_called，恒有 handoff_written 先于 run_frozen（A9 先行，逐边界三剧本）。
- `budget_turns` 是执行器的回合上限（到限即 `run_frozen{completion:limit}`）：真预算梯接入随 gate 挂剧本（S2.04+）。

### 8-3 S3.14：真适配器换入（单 Resident 全链闭环）

Stage 2 骨架的四个替换点逐一换真，剧本仍确定性（无时钟采样、无网络、无随机）：

| 替换点 | S2 骨架 | S3 换入 |
|---|---|---|
| Ledger | MemLedger | 仍 MemLedger（真 jsonl 对拍已在 conformance；换盘不增新证据） |
| 工具面 | ScriptToolSet 闭包 | 真 ToolBench（edit＋status＋exec Program 臂）对 tempdir 城根；门路由在回合层 |
| 模型 | ScriptModel 直造 ModelReturn | ScriptModel 登录 wire 形：剧本写 Anthropic wire JSON，经 gateway::dialect::response_from_wire 解成 canonical 再出 ModelReturn（翻译面进链路） |
| 时钟／配置 | 逐步 +1ms | 同＋FrozenConfig 求值（clock_stamp 三层覆盖）接入 StampGate |

**S3.14 落地记录**：`Scenario` 的 `write_domain` 字段撤销——门路由归 ToolBench（Handoff 裁定 10），域住 bench 内，executor 不再手写 domain 门。`ScriptToolSet` 未删：它仍是 `kernel::tool` 缝的第二适配器（S2.03 登记的 conformance 证据），改为**注册进真 ToolBench**，于是脚本工具与真 L0 工具走同一条门路由。波前围栏是**每波一次**而非只在 exec forecast 命中时：A14 的先行半链管的是波，`ToolBench` 内的 forecast 围栏是它在 exec 臂上的加强，两者不互相替代。空波仍提交（同树 oid），因为链可重建优于省一次提交。tool_result 的信封由 executor 挂（`pipeline::package`＋`StampGate`），与 S4 serve 同位。

事件序新增断言：edit 成功波携 checkpoint_committed（波前，断言形＝每个 tool_called 之前最近的 checkpoint_committed 晚于最近的 model_returned）；tool_result 信封可携 ClockStamp（非 Off 时）；越域写被 domain 门拒且 refusal 以 tool_result 回流；链恒可验；双跑字节对拍。“真 gateway 适配器换入”的取义：dialect 翻译面入链（纯函数，确定性保持）；endpoint 的 HTTP 面不入 sim（网络即非确定），其验证住 gateway 自身的回环假服务测试（gateway-SPEC §2）。

## 8.5 两个设计

**A（选中）：checker 复用 runtime::replay**——验证语义一处；citysim 只加「检查器」这个角色名。
**B（落选）：checker 自写链验证**——citysim 独立性更强（不依赖 runtime），但即刻成为第二验证权威，与 replay 漂移时两边都对不上夹具。落选理由：「重放与分叉共用重建器」的同一论证在此适用；citysim 依赖任何产品 crate 本就合法（第二 Main）。

## 9–16 工作流程／实现／边界／错误／依赖／硬编码／影响面／测试

- 流程：测试构造 drafts→MemLedger append→checker／conformance／对拍 JsonlLedger；S2 起另有 Scenario→run_scenario→check_chain＋事件序断言。
- 实现：append＝from_draft→canonical_line→chain_hash 推进；无别的逻辑。
- 边界：空 Ledger check 通过；单创世行通过。
- 错误：透传 kernel/replay 的 AxError，不新增码。
- 依赖：kernel（features=["conformance"]）、memory（对拍＋夹具）、runtime（复用 verify）；dev：tempfile。
- 硬编码：无。
- 影响面：S2.03 剧本执行器建于本 crate 之上；夹具脚本是 V8 起点。
- 测试：conformance 双实现、字节对拍、夹具对拍、篡改检出。

## 17 模型体验

零字节：dev-only 设施，恒不进任何 prefix。

## 18 文档同步

S2.03 落剧本执行器时增章；夹具更新须与 memory/kernel 的字节规范同一变更集。
