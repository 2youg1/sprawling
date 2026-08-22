# kernel-SPEC.md

> crate：`kernel`。本 SPEC 先于代码存在；实现不多不少地遵守本文。
> 骨架：十七节；按模块分章、每章自足。
> 施工一个模块只需读：§1–§7 总纲＋该模块章（§8-x）＋§9 以后的 crate 级各节。
> 本文记接口决策与数据面；每个模块「是什么、为什么」的短说明在它自己的模块文档头，结构面在 ARCHITECTURE.md。

## 1 需求拆解

kernel 是纯判定函数层：只吃入参吐 verdict，零内部 crate 依赖，不持有任何落盘物。Stage 1 落地九个模块（对应施工卡 S1.01–S1.06），每个可独立完成、独立验收：

| 卡 | 模块 | 形状（ARCHITECTURE §7） | 一句话 |
|---|---|---|---|
| S1.01 | `error` | 2 值类型＋6 数据面 | AxError 七字段；AxCode 35（S2 期初增 `E_STORAGE_FATAL`；P3.01 删 `E_SIGNAL_UNKNOWN`）；carrier 声明位 |
| S1.02 | `address` | 2 值类型 | 相对 city root 路径 newtype；WriteDomain 原语；reserved prefix 判定 |
| S1.02 | `locator` | 2 值类型 | `cas:`／`file:` 文法解析与呈现；fail-closed |
| S1.03 | `event` | 2 值类型 | EventKind 60（S1.03 建 55，P2.09 增 `autonomy_changed`，P4.01 增 roadmap 三件，R1.14 增 `login_started`）；in-window／record-only 二分；EventRecord 规范字节；EventRef 私有铸造 |
| S1.04 | `version` | 2 值类型 | 乐观并发：Version 单调值＋base 新鲜度判定 |
| S1.04 | `idem` | 2 值类型 | IdemKey 确定性派生（BLAKE3 XOF 16 字节＋版本字节） |
| S1.05 | `consts_external` | 6 数据面 | 外部事实常量 5 项 |
| S1.05 | `consts_policy` | 6 数据面 | 政策常量 15 项（本期落 12，3 项随类型延后，见 §8-8） |
| S1.06 | `ledger` | 3 端口 | 唯一写入口 trait；链语义（GENESIS_PREV／chain_hash）；conformance 套件 |

Stage 2 落地其余 18 个 kernel 模块（§8-10…§8-27）。**施工序＝依赖序**（S1.05 先例），与卡号序不同：

| 施工批 | 卡 | 模块 | 理由 |
|---|---|---|---|
| 骨架端口 | S2.09 | `config` `tool`(port) `model`(port) | turn 相变函数携两 port，骨架先于决断面 |
| 决断地基 | S2.05 | `taint` | 一切携 Taint 的动作依赖它 |
| 四判定 | S2.06 | `write_domain` `budget` `backpressure` `stall` | 无互依，同卡 |
| 登记与委派 | S2.07 | `goal` `repair` `delegation` `registry` | registry 供 discard 门查 Asset |
| 完成与审批 | S2.08 | `spine` `completion` `approval` | gate 的 Escalate 需 ApprovalItem |
| 隐私与删除 | S2.10 | `secret` `discard` | Egress/Discard 两门的判定输入 |
| 组合面 | S2.04 | `gate` | 五门消费上述全部，故最后 |

已有章节只加不改。

## 2 验收标准

- 每模块单测过 workspace lints（非测试代码零 unwrap/expect/panic/索引切片/裸算术/as）。
- `EventKind` 60 个 variant、`AxCode` 35 个 variant 与本文 §8-4／§8-1 表逐 variant 一致（S2 起 `xtask specalign` 机器断言，本期人工核对入卡备注）。
- 每个 EventKind 恰属 in-window／record-only 之一；in-window 恰 8 件。
- 每个 AxCode 恰有一个 carrier 声明；装载期白名单恰 5 码且封闭。
- golden EventRecord：规范字节入 insta 快照，跨平台逐字节稳定。
- proptest：Address 解析拒绝面、is_within 前缀性质、Locator 往返、IdemKey 重算不变。
- conformance 套件对任意 `impl Ledger` 可跑（S1.11 由 citysim 内存 Ledger 第二实现证明）。

Stage 2 追加：

- 类型加固十项全部有型可指；trybuild 八反例全集编译失败（S2.11）。
- kani 七 harness 入库（`#[cfg(kani)]`）：本机 Windows 无 kani 宿主支持，每条性质配 proptest 镜像本地可跑，kani 本体入 CI Linux job（CI 恢复时生效）。
- three-part refusal 矩阵：五门每条 Deny 路径的 refusal 三段非空且 alternative 可执行（S2.13）。
- conformance feature 全量导出：Ledger＋Tool＋Model 三套件（sandbox 随 S3）。

## 3 假设与歧义

1. **Locator 范围语义**：端点约定此前未定。裁定：`L<a>-<b>` 行号 1 起、闭区间（编辑器与 sed 先例）；`B<a>-<b>` 字节偏移 0 起、闭区间（HTTP Range 先例）。两者均要求 `a<=b`，`L` 另要求 `a>=1`。
2. **Address 附加拒绝面**：设计点名拒绝绝对路径、`..`、空段、非 UTF-8；本文在同一 fail-closed 精神下追加拒绝反斜杠、`.` 段、首尾 `/`、控制字符与 NUL、`:`（Windows 盘符与 NTFS ADS 两面一式拒）。放宽属「对扩展开放」，收紧后不再放回。
3. **git-oid 长度**：S3 引 git2 前按 40 位十六进制小写受理（SHA-1 仓库）；其它长度 fail-closed 拒。SHA-256 仓库支持届时按方向加长度分支。
4. **`run` 字段恒在**：city 级事件（`city_initialized`、`log_truncated` 等）无所属 Run，取 `RunId::CITY`（nil UUID）哨兵值；uuid v7 的时间戳位保证真实 Run 恒不与 nil 撞。
5. **`who` 字段本期为自由字符串**：actor 文法属 city::resident（P1）；届时收紧为类型，本文届时更新。
6. **浮点拒绝在构造点**：Ledger 载荷禁浮点（确定性七条之 6）由 `Payload::new` 与其 `Deserialize` 双侧执行，serde_json 数字非 i64/u64 可表示即拒。
7. **存储写失败码**（S2 期初定）：增装载期第 5 码 `E_STORAGE_FATAL`（AxCode 36）承载 Ledger append 等存储写失败；与 `E_CAS_CORRUPT`（读到的对象不可信）分立，recovery 相反。

## 4 现状分析

kernel 为 S0 空壳（lib.rs 仅 crate 文档）。无既有实现约束；性能敏感点唯一：chain_hash 落在单写者关键路径（选 BLAKE3 的理由），除此之外全部远离热路径。

## 5 权威信源

**改这些类型前先读 provider 官方文档**（链接已同步入 `crates/kernel/src/model.rs` 与 `crates/gateway/src/dialect.rs` 的模块注释，以便下一位先看权威再动手）：

| 主题 | 出处 |
|---|---|
| Messages API 请求与响应 | <https://platform.claude.com/docs/en/api/messages> |
| 思考块、`signature`、工具往返中的保留规则 | <https://platform.claude.com/docs/en/build-with-claude/thinking> |
| 思考强度取值 | <https://platform.claude.com/docs/en/build-with-claude/effort> |
| 什么会作废缓存断点 | <https://platform.claude.com/docs/en/build-with-claude/prompt-caching> |
| OpenAI Chat Completions 请求与响应 | <https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create/> |
| OpenAI `reasoning.effort` | <https://developers.openai.com/api/docs/guides/reasoning> |

Address；Ledger／EventRecord／IdemKey／BLAKE3；Locator；乐观并发；AxError／AxCode／carrier；reserved prefix；常量三源；类型加固十项；确定性七条；`docs/glossary.md`（词汇）；ARCHITECTURE.md §4（缝清单）、§12（kernel 模块图）、§9（七形状）、§10（确定性与硬化）。

## 6 命名统一

概念名一律英文原词：Ledger、EventRecord、EventDraft、EventRef、EventKind、in-window／record-only、Locator、CAS、B3Hash、GitOid、IdemKey、Address、reserved prefix、WriteDomain、AxError、AxCode、three-part refusal、carrier event、Seq、TimeMs、RunId、Version、conformance。
Rust variant 名取 UpperCamelCase，serde 呈现名恒为线格式拼写（EventKind 蛇形小写；AxCode `E_` 全大写）。`B3Hash` 住 `locator`（`b3-` 算法标签的文法之家），`event`／`ledger` 引用之，全库仅此一个哈希值类型。

## 7 模块边界

模块间使用关系（同 crate 内、编译器可见）：

```
error ──(carrier)──▶ event(EventKind)
event ──(载荷校验/字段)──▶ error(AxError)、address(Address)、locator(B3Hash)、consts_external(EVENT_LOG_V)
locator ──▶ address、error
address ──▶ error
idem ──▶ event(RunId, Seq)
version ──（自足）
ledger ──▶ event、error、locator(B3Hash)
```

Stage 2 新增使用边（同 crate 内）：

```
taint ──（自足）
write_domain ──▶ address、event(RunId)、consts_policy(EDIT_WAR_FREEZE)
budget ──（自足）        backpressure ──（自足）
stall ──▶ locator(B3Hash)、consts_policy(LOOP_REPEAT_THRESHOLD)
goal/repair ──▶ address、event(RunId)
delegation ──（自足）     registry ──▶ locator、event(EventRef)、error
spine ──▶ locator、completion(Progress)、consts
completion ──▶ event(EventRef/EventKind)、budget(BudgetUse)
approval ──▶ registry(ResidentId)、locator、event(TimeMs)、taint、consts_policy
config ──（自足）         tool ──▶ address、event(Payload)、error
model ──▶ locator(B3Hash)、event(Payload)、tool(ToolCall)
secret ──▶ consts_external(SECRET_SHAPES)、consts_policy(SECRET_ENTROPY_MIN)、error
discard ──▶ address、locator、taint、budget(ByteLen)、registry、tool(ExecArm)、consts_policy
gate ──▶ 上述全部（组合面）＋idem
```

`error` 与 `event` 互相引用（error 声明 carrier 需 EventKind；event 载荷校验产 AxError）——同 crate 内合法，且二者本就是同一冻结面（C8）的两半，不视为耦合事故。

**本 crate 不做什么（否定式三条）**：
- 不做 I/O、不采样时钟、不生成随机数——RunId／时间戳／种子全部由调用方注入；`uuid` 依赖仅用于解析与格式化，恒不启用生成特性。
- 不实现任何端口——`Ledger` 的实现住 memory 与 citysim；kernel 只声明 trait 与链语义纯函数。
- 不认识文件系统、明文凭证与颜色——canonicalize、Vault、OKLCH 各归其效果面模块。

## 8 接口先行（按模块分章）

### 8-1 kernel::error（S1.01）

**类型**：

```rust
#[non_exhaustive]                       // C8：对扩展开放
pub enum AxCode { PathNotFound, /* …35 variant，serde 呈现名见下表 */ }

pub struct GateRefusal {                // three-part refusal；三段必填
    rule: String, violation: String, alternative: String,
}

pub struct AxError {                    // 七字段，序列化字段序＝声明序
    code: AxCode, action: String, subject: String,
    nearby: Vec<String>, recovery: String, retriable: bool,
    gate: Option<GateRefusal>,
}

pub enum Carrier { Event(EventKind), Loadtime }
impl AxCode {
    pub fn carrier(&self) -> Carrier;   // 穷尽 match，无 catch-all；唯一声明位
    pub fn as_str(&self) -> &'static str; // "E_…" 拼写，serde 与 Display 共用
}
```

**构造**：字段私有；两个构造子＋组合子，使「Gate 拒绝码 ⇒ gate 三段在场」由构造路径保证：

```rust
impl AxError {
    /// Non-gate failure. `retriable` defaults to false (fail-closed).
    pub fn failure(code: AxCode, action: impl Into<String>, subject: impl Into<String>) -> Self;
    /// Gate refusal. Sets `gate` to the mandatory three parts.
    pub fn refusal(code: AxCode, action: impl Into<String>, subject: impl Into<String>, gate: GateRefusal) -> Self;
    pub fn with_nearby(self, nearby: Vec<String>) -> Self;
    pub fn with_recovery(self, recovery: impl Into<String>) -> Self;
    pub fn retriable(self) -> Self;     // 显式声明可重试，默认不可
    pub fn code(&self) -> &AxCode;  pub fn gate(&self) -> Option<&GateRefusal>;
}
```

「gate 码走 `refusal`」本期是构造纪律＋单测；S2 `kernel::gate` 是全库唯一 gate 码生产者，citysim 不变量 8 号在系统层复验。derive `Serialize/Deserialize`（Ledger 载荷需要）、`Clone/Debug/PartialEq`；`thiserror::Error` 提供 Display（`{code}: {action} on {subject}`）。

**AxCode 35 全集与 carrier 对应（specalign 数据面）**

> P3.01：协作组由六降为五——`E_SIGNAL_UNKNOWN` 已定义掉（三码之一；理由与实测见 `collab-SPEC.md` §8-1）。删除时全仓只有本文件提到它，零生产者。剩下两码（`E_WORKTREE_BUSY`／`E_DIGEST_SUSPECT`）已在各自 SPEC 里答过「能否定义掉」，答案是能保留——它们各自有一个真实的运行期情境。


| 组 | AxCode | carrier event |
|---|---|---|
| 基表 | `E_PATH_NOT_FOUND` | `tool_result` |
| 基表 | `E_TOOL_UNKNOWN` | `tool_result` |
| 基表 | `E_TOOL_UNAVAILABLE` | `tool_result` |
| 基表 | `E_INVALID_ARGS` | `tool_result` |
| 基表 | `E_OUTSIDE_WRITE_DOMAIN` | `gate_denied` |
| 基表 | `E_VERSION_CONFLICT` | `tool_result` |
| 基表 | `E_GATE_DENIED` | `gate_denied` |
| 基表 | `E_BUDGET_EXHAUSTED` | `budget_limit` |
| 基表 | `E_TIMEOUT` | `tool_result` |
| 基表 | `E_PROVIDER` | `provider_degraded` |
| 基表 | `E_EVIDENCE_MISSING` | `tool_result` |
| 基表 | `E_LOOP_SUSPECTED` | `watchdog_fired` |
| 基表 | `E_LOCATOR_INVALID` | `tool_result` |
| 基表 | `E_SANDBOX_DENIED` | `tool_result` |
| 协作 | `E_DRAFT_STALE` | `tool_result` |
| 协作 | `E_GOAL_CONFLICT` | `tool_result` |
| 协作 | `E_TAINTED_ACTION` | `gate_denied` |
| 协作 | `E_REPAIR_BUSY` | `tool_result` |
| 协作 | `E_DELEGATION_DEPTH` | `gate_denied` |
| 治理与设施 | `E_APPROVAL_PENDING` | `approval_requested` |
| 治理与设施 | `E_APPROVAL_DENIED` | `approval_resolved` |
| 治理与设施 | `E_CROSS_BUILDING_DENIED` | `gate_denied` |
| 治理与设施 | `E_DIGEST_SUSPECT` | `tool_result` |
| 治理与设施 | `E_CREDENTIAL_MISSING` | `tool_result` |
| 治理与设施 | `E_CONFIG_INVALID` | 装载期（无 carrier） |
| 治理与设施 | `E_CAS_CORRUPT` | 装载期（无 carrier） |
| 治理与设施 | `E_STORAGE_FATAL` | 装载期（无 carrier） |
| 治理与设施 | `E_WORKTREE_BUSY` | `tool_result` |
| 治理与设施 | `E_BROWSER_UNAVAILABLE` | `tool_result` |
| 治理与设施 | `E_ENDPOINT_DIALECT_UNSUPPORTED` | `endpoint_lost` |
| 治理与设施 | `E_WIRE_MISMATCH` | 装载期（无 carrier） |
| 治理与设施 | `E_LOG_VERSION_UNSUPPORTED` | 装载期（无 carrier） |
| 隐私与 Discard | `E_SECRET_EGRESS` | `gate_denied` |
| 隐私与 Discard | `E_DISCARD_IRREVERSIBLE` | `gate_denied` |
| 运行未知 | `E_TOOL_OUTCOME_UNKNOWN` | `tool_result` |

装载期五码（`E_CONFIG_INVALID` `E_CAS_CORRUPT` `E_STORAGE_FATAL` `E_WIRE_MISMATCH` `E_LOG_VERSION_UNSUPPORTED`）＝C9 唯一例外白名单，封闭且不得增长（第 5 码于 S2 期初增补）；`Carrier::Loadtime` 即其类型面。
两条呈现约束：`E_SECRET_EGRESS` 的 subject 只写 SecretRef 与位置、恒不回显命中字节；`E_DISCARD_IRREVERSIBLE` 的 alternative 必须可执行。执行点在各生产模块（S2），此处记为 carrier 表随附契约。

**carrier() 落地时机**：`Carrier` 依赖 `EventKind`，故 `carrier()` 与 S1.03（event）同卡落码；S1.01 先落本表与其余全部（AxError／GateRefusal／AxCode／serde／构造子）。

### 8-2 kernel::address（S1.02）

```rust
pub struct Address(String);             // 不变量在唯一构造点强制；无 setter
pub const RESERVED_PREFIX: &str = ".sprawling";

impl Address {
    /// Sole constructor. Grammar: relative, `/`-separated, segments of
    /// non-control UTF-8; rejects absolute (incl. drive/UNC), `..`, `.`,
    /// empty segments, backslash, NUL/control, leading/trailing `/`.
    pub fn parse(raw: &str) -> Result<Self, AxError>;   // E_INVALID_ARGS
    pub fn is_within(&self, prefix: &Address) -> bool;  // 段边界字节前缀；WriteDomain 原语
    pub fn is_reserved(&self) -> bool;                  // 首段 == RESERVED_PREFIX（C17）
    pub fn as_str(&self) -> &str;
}
```

- 派生：`Clone/Debug/Display/PartialEq/Eq/PartialOrd/Ord/Hash`（BTreeMap 键）。
- serde：呈现为字符串；`Deserialize` 经 `parse` 复验（fail-closed 读盘）。
- Windows 按字节比较、不折叠大小写；`is_within` 自反（`a.is_within(a)`）。
- 符号链接 canonicalize 属效果面（S2 write_domain 的适配层）；本原语只对已规范化相对路径作证。
- 解析拒绝的 AxError：`action="parse address"`、`subject=原串`、recovery 指出违规成分与合法形态。

### 8-3 kernel::locator（S1.02）

```rust
pub struct B3Hash([u8; 32]);            // 全库唯一哈希值类型；hex64 小写呈现
pub struct GitOid([u8; 20]);            // 40 位十六进制小写；serde 与 B3Hash 同形（S4.02 增）
pub enum Range { Lines { from: u64, to: u64 },   // 1 起、闭区间
                 Bytes { from: u64, to: u64 } }  // 0 起、闭区间（HTTP Range 先例）
pub enum Locator {
    Cas  { hash: B3Hash, range: Option<Range> },
    File { address: Address, oid: GitOid, range: Option<Range> },
}
impl Locator {
    /// Fail-closed: anything not exactly the grammar is E_LOCATOR_INVALID.
    /// Unknown scheme or algorithm tag is an error, never a fallback path.
    pub fn parse(raw: &str) -> Result<Self, AxError>;
}
impl fmt::Display for Locator { /* 规范拼写往返：parse(x).to_string() == 规范形 */ }
```

- 文法：`cas:b3-<hex64>[#(L|B)<a>-<b>]`｜`file:<address>@<hex40>[#(L|B)<a>-<b>]`。`file:` 以最后一个 `@` 切分（address 段内允许 `@`，oid 恒不含）。
- **规范回声断言**：解析成功后另断言 `Display(结果) == 原串`，不等即 `E_LOCATOR_INVALID`（recovery 给出规范拼写）——一条规则封死大写 hex、前导零、`+` 号等全部非规范变体。
- 十六进制恒小写（规范字节唯一化）；大写拒。`b3-` 外的算法标签拒（对扩展开放：新标签＝新 variant，旧解析不宽容）。
- `SecretRef`（`secret:`）恒不入本文法——`secret:` 前缀命中即 `E_LOCATOR_INVALID`，两套解析器分立（类型层理由）。
- serde：字符串形（Display/parse 往返）。
- `B3Hash::from_bytes([u8;32])`／`to_hex()`；`Range` 构造校验 `from<=to`（Lines 另 `from>=1`）。
- S2 增 `B3Hash::digest(bytes: &[u8]) -> B3Hash`（blake3 直算）：全库内容哈希的唯一产地——prefix 分段哈希与 stall 指纹均经此，不在 kernel 外直呼 blake3（一个哈希一个家）。`chain_hash` 保留为链语义专名（内部改经 digest）。

### 8-4 kernel::event（S1.03）

```rust
pub struct RunId(Uuid);                 // uuid v7 仅人读标识；kernel 不生成
impl RunId { pub const CITY: RunId;     // nil UUID：city 级事件哨兵
             pub fn from_bytes([u8;16]) -> Self;  pub fn parse(&str) -> Result<Self, AxError>; }
pub struct Seq(u64);
impl Seq { pub const FIRST: Seq;        // 0；创世行
           pub fn next(self) -> Result<Seq, AxError>; }   // checked_add
pub struct TimeMs(u64);                 // UTC 整数毫秒；只入参不采样

#[non_exhaustive]
pub enum EventKind { CityInitialized, /* …55 variant，serde 蛇形 */ }
pub enum WindowClass { InWindow, RecordOnly }
impl EventKind {
    pub fn window_class(&self) -> WindowClass;  // 穷尽 match；二分权威
    pub const ALL: [EventKind; 55];             // specalign 与计数断言的数据面
}

pub struct Payload(serde_json::Map<String, Value>);
impl Payload {
    /// Sole constructor: rejects any float anywhere in the tree
    /// (determinism rule 6). Deserialize re-validates on read.
    pub fn new(map: Map<String, Value>) -> Result<Self, AxError>;  // E_INVALID_ARGS
    pub fn empty() -> Self;
}

pub struct EventDraft {                 // 调用方给的一半：语义内容
    pub run: RunId, pub t: TimeMs, pub who: String,
    pub addr: Option<Address>, pub kind: EventKind, pub data: Payload,
    pub ig: bool,                       // 「可忽略」标记；写方默认 false
}
pub struct EventRecord { /* v, run, seq, prev, t, who, addr, kind, data, ig —— 字段私有 */ }
impl EventRecord {
    /// Adapter-side assembly: the Ledger implementation owns seq/prev/v.
    pub fn from_draft(draft: EventDraft, seq: Seq, prev: B3Hash) -> Self;  // v ＝ EVENT_LOG_V
    /// Canonical bytes: serde_json, struct field order = declaration order,
    /// payload keys sorted (serde_json BTreeMap), no trailing newline.
    pub fn canonical_line(&self) -> Result<Vec<u8>, AxError>;
    pub fn to_ref(&self) -> EventRef;   // 铸造需持有整条记录
    pub fn parse_line(raw: &[u8]) -> Result<Self, AxError>;  // 读侧：逐字段复验
    pub fn seq(&self) -> Seq;  pub fn kind(&self) -> EventKind;  pub fn v(&self) -> u32;
}
pub struct EventRef { seq: Seq, kind: EventKind }   // 字段私有；无公开构造子
```

- 序列化细节：`addr` 为 None 与 `ig` 为 false 时省略键；其余八键恒在；键序＝声明序 `v,run,seq,prev,t,who,addr,kind,data,ig`。此即 V8 跨平台字节一致的规范。
- 铸造纪律（15.3-1）：`EventRef` 唯二铸造路径＝Ledger append 流程（适配器持刚组装的 EventRecord 调 `to_ref`）与 replay 验链后逐条 `to_ref`。字段私有使字面量伪造编译不过（S2.11 trybuild 反例）。
- `parse_line` 是读侧唯一入口：serde 反序列化＋Payload 复验；未知 kind 在此报错（呈现语义见 runtime::replay 章——携 `ig` 的行例外）。

**EventKind 60 全集与二分（specalign 数据面；「入窗」＝InWindow，共 8）**：

| 组 | kind | 窗类 |
|---|---|---|
| 创世与空间 | `city_initialized` | record-only（创世行，prev＝64 个 0） |
| 创世与空间 | `building_created` | record-only |
| 基集 | `run_started` | record-only |
| 基集 | `run_forked` | record-only |
| 基集 | `prompt_assembled` | **in-window** |
| 基集 | `model_called` | **in-window** |
| 基集 | `model_returned` | **in-window** |
| 基集 | `tool_called` | **in-window** |
| 基集 | `tool_result` | **in-window** |
| 基集 | `result_offloaded` | **in-window** |
| 基集 | `gate_checked` | record-only |
| 基集 | `gate_denied` | record-only |
| 基集 | `checkpoint_committed` | record-only |
| 基集 | `handoff_written` | record-only |
| 基集 | `steer_received` | **in-window** |
| 基集 | `cancel_received` | record-only |
| 基集 | `watchdog_fired` | record-only |
| 基集 | `budget_limit` | record-only |
| 基集 | `run_frozen` | record-only |
| 基集 | `log_truncated` | record-only |
| 协作 | `signal_enqueued` | record-only |
| 协作 | `signal_consumed` | **in-window** |
| 协作 | `draft_held` | record-only |
| 协作 | `draft_resolved` | record-only |
| 协作 | `goal_registered` | record-only |
| 协作 | `goal_conflict` | record-only |
| 协作 | `arbitration_verdict` | record-only |
| 协作 | `repair_started` | record-only |
| 协作 | `repair_reused` | record-only |
| 协作 | `worktree_opened` | record-only |
| 协作 | `pr_opened` | record-only |
| 协作 | `pr_merged` | record-only |
| 协作 | `pr_rejected` | record-only |
| 协作 | `roadmap_claimed` | record-only |
| 协作 | `roadmap_finished` | record-only |
| 协作 | `roadmap_released` | record-only |
| 治理与设施 | `approval_requested` | record-only |
| 治理与设施 | `approval_resolved` | record-only |
| 治理与设施 | `policy_created` | record-only |
| 治理与设施 | `policy_revoked` | record-only |
| 治理与设施 | `taint_promoted` | record-only |
| 治理与设施 | `cross_building_transfer` | record-only |
| 治理与设施 | `takeover_started` | record-only |
| 治理与设施 | `rollback_applied` | record-only |
| 治理与设施 | `city_halted` | record-only |
| 治理与设施 | `backpressure_shed` | record-only |
| 治理与设施 | `digest_invalidated` | record-only |
| 治理与设施 | `endpoint_attached` | record-only |
| 治理与设施 | `endpoint_lost` | record-only |
| 治理与设施 | `model_selected` | record-only |
| 治理与设施 | `provider_degraded` | record-only |
| 治理与设施 | `login_started` | record-only（R1.14；订阅登录开始，载荷携 provider 与授权 URL——URL 里只有 PKCE challenge 与 state，恒无凭证） |
| 治理与设施 | `eval_run` | record-only |
| 治理与设施 | `asset_archived` | record-only |
| 治理与设施 | `credential_lent` | record-only |
| 隐私与 Discard | `secret_captured` | record-only（行内无明文无哈希前缀） |
| 隐私与 Discard | `secret_egress_blocked` | record-only |
| 隐私与 Discard | `file_discarded` | record-only |
| 隐私与 Discard | `discard_restored` | record-only |
| 隐私与 Discard | `autonomy_changed` | record-only |

二分判据唯一：该事件载荷是否决定模型请求字节；不存在第三类。

### 8-5 kernel::version（S1.04）

```rust
pub struct Version(u64);
impl Version { pub const FIRST: Version;               // 1；首个可见版本
               pub fn next(self) -> Result<Version, AxError>; }
pub enum VersionVerdict { Fresh, Stale { current: Version } }
/// Optimistic-concurrency primitive: pure verdict, no bool.
pub fn check_base(current: Version, base: Version) -> VersionVerdict;
```

`Stale` 到 `E_VERSION_CONFLICT`＋新鲜 diff 的映射在 runtime::tools::edit（S3）：kernel 只判新鲜度，不认识 diff。`base > current` 同样 `Stale`（唯一真版本是 current；超前的 base 是调用方脑补）。

### 8-6 kernel::idem（S1.04）

```rust
pub struct IdemKey { v: u8, digest: [u8; 16] }   // 私有；无 From<Uuid>、无 Default、无随机
pub const IDEM_DERIVE_V: u8 = 1;
impl IdemKey {
    /// Deterministic dedup key for outward actions:
    /// BLAKE3-XOF 16 bytes over `run(16B) || seq(8B LE) || action_canonical`.
    /// Fixed-width prefix makes the framing injective; same inputs after
    /// resume/replay re-derive the identical key.
    pub fn derive(run: &RunId, seq: Seq, action_canonical: &[u8]) -> IdemKey;
}
// 定义点仍在 kernel::idem；取关联函数而非自由函数，避免裸名 `derive` 入 crate 门面。
impl fmt::Display for IdemKey { /* "idem<v>-<hex32>" */ }
```

serde：字符串形。动作规范化（action_canonical 的构造规则）属 S2 工具面；本模块只定派生函数与框架。

### 8-7 kernel::consts_external（S1.05）

外部事实 5 项（改它＝外界变了）：

```rust
pub const CACHE_BREAKPOINTS_MAX: u32 = 4;
pub const PROMPT_CACHE_TTL_SECS: u64 = 300;
pub const EVENT_LOG_V: u32 = 1;                  // EventRecord.v 的唯一来源
pub const L0_TOOLS: [&str; 3] = ["exec", "edit", "status"];
pub struct SecretShape { pub provider: &'static str, pub prefix: &'static str,
                         pub charset: SecretCharset, pub len: (u16, u16) }   // 闭区间
pub enum SecretCharset { Base62, Base64Url, HexLower, Base36Lower }
pub const SECRET_SHAPES: [SecretShape; N] = [ /* 公开 provider 令牌形状，见 §14 */ ];
```

`SECRET_SHAPES` 是数据不是代码（零分支）；消费者是 S2 `kernel::secret::scan` 与 `xtask secret`。

### 8-8 kernel::consts_policy（S1.05）

政策常量（改它须 EVAL 证据或一次裁决）。比值以整数对表示（kernel 判定路径禁浮点，C10/16.3-6）：

```rust
pub struct Ratio { pub num: u32, pub den: u32 }   // 分子/分母；恒不约简，拼写即裁定值
pub const STARTUP_BUDGET_TOKENS: u64 = 2000;
pub const CTX_REMINDER_RATIO: Ratio = Ratio { num: 1, den: 2 };      // 0.5
pub const LOOP_REPEAT_THRESHOLD: u32 = 3;
pub const OFFLOAD_MIN_BYTES: u64 = 16_384;
pub const DRAFT_HELD_ESCALATE: u32 = 3;
pub const EDIT_WAR_FREEZE: u32 = 2;
pub const SECRET_ENTROPY_MIN: Ratio = Ratio { num: 7, den: 2 };      // 3.5 bits/char
pub const DISCARD_FILES_MAX: u32 = 16;
pub const DISCARD_BYTES_MAX: u64 = 1_048_576;
pub const DISCARD_RETENTION_DAYS: u32 = 30;
pub const POLICY_IDLE_DAYS: u32 = 90;
pub const CLOCK_ZONES_MAX: u32 = 4;
pub const WORKTREE_MAX_BYTES: u64 = 2_147_483_648;                   // 2 GiB（P2.03）
```

`WORKTREE_MAX_BYTES` 是上限而非磁盘余量探测：余量是一台机器当下的事实，上限则是一句拒绝说得出、一个人改得动的数；建树前校，故一座过大的城是被拒而不是被拷到一半（`memory::worktree`）。

16 项中 3 项随类型延后（表先行、值后到，位置恒在本模块）：`AUTONOMY_DEFAULT`（需 `Autonomy`，S2 approval 卡落）；`CLOCK_STAMP_DEFAULT`（需时钟档枚举；该枚举住 kernel 何处属 S2 config 卡决策——kernel 不得依赖 runtime）；`SUBAGENT_CTX_LOCK_DEFAULT`（未给数值，S2 budget 卡携证据定值）。三项落地前，本模块不提供任何替身值。

### 8-9 kernel::ledger（S1.06；缝清单文件，全库五真缝之一）

```rust
/// The only write entrance to history (ARCHITECTURE §1-2). Implementations
/// own seq/prev assignment and byte production; callers never serialize.
/// Contract: Ok(ref) ⇒ the record is durable in that adapter's medium and
/// `ref` points at it; Err ⇒ nothing observable was appended (torn bytes
/// are the reopen path's business, not the caller's).
pub trait Ledger {
    fn append(&mut self, draft: EventDraft) -> Result<EventRef, AxError>;
}

pub const GENESIS_PREV: B3Hash;                       // 32 个零字节（hex64 全 0）
/// Chain rule: prev of line k+1 = blake3(raw bytes of
/// line k, excluding the line terminator). One hash function, one home.
pub fn chain_hash(raw_line: &[u8]) -> B3Hash;

#[cfg(feature = "conformance")]
pub mod conformance {
    /// Read-back surface for verification only; production callers never read
    /// through the Ledger handle (projections do). Lives behind the feature
    /// so the production port stays write-only.
    pub trait LedgerInspect { fn raw_lines(&self) -> Result<Vec<Vec<u8>>, AxError>; }
    /// One assertion suite for every implementation (V3). `fresh` must yield
    /// an empty ledger each call.
    pub fn assert_ledger_conformance<L: Ledger + LedgerInspect>(fresh: impl FnMut() -> L);
}
```

conformance 六断言（对任意实现同一套）：
1. 首条 append 得 `Seq::FIRST`，记录 prev＝`GENESIS_PREV`；
2. seq 连续无洞（逐条 +1）；
3. 链续：第 k 行 prev＝`chain_hash(第 k-1 行原始字节)`；
4. 写方规范：每行 `parse_line` 后 `canonical_line` 与原始字节逐字节相等；
5. `v` 恒＝`EVENT_LOG_V`，`ref.kind`＝draft.kind；
6. 确定性：同一 draft 序列灌两个 fresh 实例，raw_lines 逐字节相同。

### 8-10 kernel::taint（S2.05）

```rust
pub struct TaintSource(String);            // 非空来源标签（如 "web:example.com"）；文法 P1 随 Endpoint 收紧
pub struct TaintSet(BTreeSet<TaintSource>); // 空集＝内生数据；并集半格
impl TaintSet { pub fn empty() -> Self;  pub fn union(&self, other: &TaintSet) -> TaintSet;
                pub fn is_empty(&self) -> bool;  pub fn contains(&self, s: &TaintSource) -> bool; }

pub struct Tainted<T> { /* value, taint —— 字段私有 */ }
impl<T> Tainted<T> {
    /// Sole entrance for external content. Custody
    /// composition (secret scan before CAS) is the effect layer's wiring
    /// at this call site (S3); the type itself stays pure.
    pub fn new(value: T, source: TaintSource) -> Self;
    pub fn peek(&self) -> &T;                                   // 借用读，拿不走所有权
    pub fn map<U>(self, f: impl FnOnce(&T) -> U) -> Tainted<U>; // 派生：同集保持
    pub fn join<U, V>(self, other: Tainted<U>, f: impl FnOnce(&T, &U) -> V) -> Tainted<V>; // 并集
    pub fn taint(&self) -> &TaintSet;
}
```

- **无解包面**：无 `into_inner`、无 `Deref`、字段私有——「摘干净再传下游」编译不过（trybuild 反例，S2.11）。`map` 取 `FnOnce(&T)`（借用入参），闭包无法把所有权搬出环外。
- **Tainted 恒不 serde**：`Deserialize` 即第二构造入口，伪造空 Taint 即洗白；`TaintSet` 可 serde（事件载荷需要来源清单）。
- kani：`join` 输出 taint ⊇ 两入参（并集单调不丢）；proptest 镜像同性质（本机无 kani 宿主支持，CI Linux 跑）。
- 上游错误文本、摘要继承、动作构造器强制并集：均在消费方模块（discard/approval/gate 本期；pipeline/digest S3）逐处落实，本模块只供类型。

### 8-11 kernel::write_domain（S2.06）

```rust
pub struct WriteDomain { /* prefixes: BTreeSet<Address> —— 私有 */ }
impl WriteDomain {
    /// C17 at the construction point: any reserved-prefix member is refused.
    pub fn new(prefixes: Vec<Address>) -> Result<Self, AxError>;   // E_INVALID_ARGS
    pub fn admits(&self, target: &Address) -> DomainVerdict;
    pub fn prefixes(&self) -> impl Iterator<Item = &Address>;
}
#[non_exhaustive]
pub enum DomainVerdict { Within, Outside { prefixes: Vec<String> } }  // prefixes 供 three-part 的 nearby

pub struct EditSample { pub addr: Address, pub run: RunId }            // 切片序＝时序
#[non_exhaustive]
pub enum EditWarVerdict { Calm, Freeze { addr: Address } }
pub fn observe_edit_war(samples: &[EditSample]) -> EditWarVerdict;
```

- `admits`：目标 `is_reserved()` 恒 Outside（构造点已拒，判定点再拒＝fail-closed 双层）；否则 ∃prefix 使 `target.is_within(prefix)` → Within。空前缀集合法（只读角色），恒 Outside。
- **edit war 判据**：同 addr 的样本按序去重相邻同 Run 后得 run 序列 r₁…rₙ；「夺回」＝rᵢ==rᵢ₋₂ 且 rᵢ≠rᵢ₋₁；夺回数 ≥ `EDIT_WAR_FREEZE`(2) → Freeze（A→B→A→B 即两次夺回）。逐 addr 独立计，首个达阈的 addr 入 verdict（BTreeMap 序）。
- kani：reserved 目标恒不 Within；`admits` 全函数无 panic。

### 8-12 kernel::budget（S2.06）

```rust
pub struct UsdMicros(u64);  pub struct Tokens(u64);  pub struct ByteLen(u64);   // 15.3-6 钱与量整数化，三新型同家
// 各：pub const fn new(u64) / pub const fn get() / pub fn checked_add(self, o) -> Option<Self>
// checked_add 取 Option 而非 Result：溢出的裁决权归调用点（spend 门读作 Exhausted，其余读作 E_INVALID_ARGS），
// 在原语层预先选一个错误故事会迫使 spend 门反封 AxError。

pub struct BudgetCap { pub usd: UsdMicros, pub tokens: Tokens }
pub struct BudgetUse { pub usd: UsdMicros, pub tokens: Tokens }    // serde（Progress::Unplanned 载荷）
pub struct BudgetLevel { pub cap: BudgetCap, pub used: BudgetUse }
pub struct BudgetLadder { pub city: BudgetLevel, pub building: BudgetLevel, pub run: BudgetLevel }
#[non_exhaustive] pub enum BudgetLayer { City, Building, Run }
#[non_exhaustive] pub enum SpendVerdict { Admit, Exhausted { layer: BudgetLayer } }
/// Total function: u64 overflow means the spend exceeds any representable
/// remainder, hence Exhausted (fail-closed), never a panic or an error path.
pub fn admit_spend(ladder: &BudgetLadder, cost: &BudgetUse) -> SpendVerdict;

pub struct CtxLock(Tokens);   // pub const fn new
#[non_exhaustive] pub enum CtxVerdict { Within, Reached }
pub fn observe_ctx(used: Tokens, lock: CtxLock) -> CtxVerdict;     // used ≥ lock → Reached
```

- 逐层判 `used + cost ≤ cap`（usd 与 tokens 分别判，任一超即该层 Exhausted）；报最内层先超者（Run→Building→City 序检查，报首个）。
- 耗尽不是错误是审批：SpendVerdict 不携 AxError；gate::spend 把 Exhausted 塑成 Escalate(BudgetLimit)。
- kani：任意 u64 输入无 panic、无回绕；`Admit ⇒ 逐层不超`。
- `SUBAGENT_CTX_LOCK_DEFAULT: Tokens` 随本卡落 consts_policy，取值与证据见 §14。

### 8-13 kernel::backpressure（S2.06）

```rust
pub struct QueueStats { pub depth: u64, pub capacity: u64 }
pub struct ItemMeta { pub cost: u64 }        // 槽位数：Signal＝1，受理新 Run 的 fd 预留可 >1
#[non_exhaustive] pub enum ShedReason { CapacityExhausted }
#[non_exhaustive] pub enum Admission { Admit, Shed { reason: ShedReason } }
/// Decides whether the queue admits one more item. Pure and total:
/// depth + cost ≤ capacity admits; checked arithmetic, overflow sheds.
pub fn admit(stats: &QueueStats, item: &ItemMeta) -> Admission;
```

- 削峰是 city-wide 准入姿态：同一函数服务 Signal 队列与 fd 预留（capacity 语义由调用方赋）；队列与计数器住 memory::queue（S3）与调用方。
- kani：全函数无溢出；单调性——同 capacity/cost 下 depth 更小恒不更难 Admit。
- 饱饱不饿死（活性）属 citysim liveness（P2），非本函数可证。

### 8-14 kernel::stall（S2.06）

```rust
pub struct ActionFingerprint(B3Hash);        // 动作规范字节的摘要；derive(bytes) 内调 B3Hash::digest
#[non_exhaustive] pub enum StallVerdict { Ok, Stall { repeats: u32 } }
/// Sole stall criterion. Sample = recent fingerprints
/// in time order; a tail run of identical prints ≥ LOOP_REPEAT_THRESHOLD
/// is a stall. Counters and queues live with the caller, never here.
pub fn observe(recent: &[ActionFingerprint]) -> StallVerdict;
```

- 判尾部连续：历史中早先的重复不算（已被新动作打断＝已恢复）。阬值取 `LOOP_REPEAT_THRESHOLD`(3)。
- watchdog（S3）只消费 verdict 不转发判据；`E_LOOP_SUSPECTED` 的塑形在处置面。

### 8-15 kernel::goal（S2.07）

```rust
pub struct GoalId(String);                   // 非空
#[non_exhaustive] pub enum GoalResource { Path(Address), External(String) }  // External 非空（外部不可分资源名）
pub struct GoalEntry { pub id: GoalId, pub owner: String, pub resources: Vec<GoalResource>,
                       pub statement: String, pub standing: bool }
#[non_exhaustive] pub enum GoalVerdict { Clear, Conflict { with: GoalId } }
/// Same-resource mutual exclusion only: detection is
/// kernel's, arbitration is not. Paths conflict on prefix overlap either
/// way; External conflicts on equality; Path vs External never.
pub fn detect_conflict(registered: &[GoalEntry], candidate: &GoalEntry) -> GoalVerdict;
```

- 报首冲突（registered 切片序，确定）；id 去重归登记方（调用方持表）；candidate 自冲突不判（同 owner 同 id 重提交属幂等）。
- `E_GOAL_CONFLICT` 的塑形在注册回传（S3 工具面）；kernel 只出 verdict。

### 8-16 kernel::repair（S2.07）

```rust
#[non_exhaustive] pub enum RepairVerdict { Lease, Queued { holder: RunId } }
/// One live lease per scope subtree: overlap either
/// way queues; the same holder re-requesting its exact scope re-leases
/// (idempotent). State (the active map) lives with the caller.
pub fn request(active: &BTreeMap<Address, RunId>, scope: &Address, who: &RunId) -> RepairVerdict;
```

- 重叠判据同 goal：`scope.is_within(s) || s.is_within(scope)`；报首个重叠的 holder（BTreeMap 序确定）。

### 8-17 kernel::delegation（S2.07）

```rust
#[non_exhaustive] pub enum DelegateKind { Resident, Ephemeral }
/// Depth-zero position; the only type with a delegate method (15.3-10).
pub struct Delegator(/* 私有单元 */);
impl Delegator { pub fn root() -> Delegator;                     // 铸造点：装配/citysim
                 pub fn delegate(&self, kind: DelegateKind) -> Delegate; }
pub struct Delegate { /* kind —— 私有 */ }                        // 无 delegate 方法：trybuild 反例
impl Delegate { pub fn kind(&self) -> &DelegateKind; }

#[non_exhaustive] pub enum Depth { Root, Delegated }
#[non_exhaustive] pub enum DelegationVerdict { Allow, Deny }
/// Dynamic half of the two-layer guard (static half = the missing method).
pub fn admit(parent: Depth, kind: &DelegateKind) -> DelegationVerdict;   // Delegated 恒 Deny
```

- `root()` 公开是诚实的承认：类型封的是「从 Delegate 值铸子代」这条路，「谁持有 Delegator」由装配纪律看守；动态 `admit` 是第二层（「深度两层拦」）。
- `E_DELEGATION_DEPTH` 不消解；塑形在 gate::spawn（gate 码唯一生产者）。

### 8-18 kernel::registry（S2.07）

```rust
pub struct ResidentId(String);               // 非空；`role@building.n` 文法 P1 随 city::resident 收紧
pub struct Claim { pub locator: Locator, pub by: String }        // 证词：未验证产出
pub struct Artifact { /* locator, verified_by —— 私有 */ }
impl Artifact {
    /// Sole constructor: player–referee in the type. Verification evidence
    /// must be a tool_result or model_returned ref, else E_EVIDENCE_MISSING.
    pub fn verify(claim: Claim, evidence: EventRef) -> Result<Artifact, AxError>;
    pub fn locator(&self) -> &Locator;  pub fn verified_by(&self) -> &EventRef;
}

pub struct Registry { /* artifacts: BTreeMap<String, Artifact>, assets: BTreeSet<String>,
                         residents: BTreeSet<ResidentId> —— 私有 */ }
#[non_exhaustive] pub enum RegisterVerdict { Registered, AlreadyRegistered }
impl Registry {
    pub fn new() -> Registry;
    pub fn register_artifact(&mut self, artifact: Artifact) -> RegisterVerdict;   // 键＝locator 规范拼写
    pub fn promote_asset(&mut self, locator: &Locator) -> Result<RegisterVerdict, AxError>; // 未登记 → E_PATH_NOT_FOUND
    pub fn register_resident(&mut self, id: ResidentId) -> RegisterVerdict;
    pub fn artifact(&self, locator: &Locator) -> Option<&Artifact>;
    pub fn is_asset(&self, locator: &Locator) -> bool;           // Discard 门的查询面
}
```

- Registry 是值不是存储：状态住调用方（S3 起由 projection 重建）；kernel 只定登记规则与查询面。
- 评分归 eval（P3）；promotion 本期只登记不评分。

### 8-19 kernel::spine（S2.08）

```rust
#[non_exhaustive] pub enum RoadmapStatus { NotStarted, InProgress, Done, Blocked, AwaitingApproval }
pub const ROADMAP_STATUS_SPELLINGS: [(RoadmapStatus, &str); 5];   // Not started｜In progress｜Done｜Blocked｜Awaiting approval（模板拼写，单套）
#[non_exhaustive] pub enum EvidenceCell { Empty, Invalid { raw: String }, Present(Locator) }
pub struct RoadmapRow { pub index: u64, pub item: String, pub status: RoadmapStatus, pub evidence: EvidenceCell }
#[non_exhaustive] pub enum RoadmapShape { WellFormed { rows: Vec<RoadmapRow> }, Malformed { problems: Vec<String> } }
pub fn check_roadmap_shape(text: &str) -> RoadmapShape;
pub fn set_roadmap_status(text: &str, index: u64, status: RoadmapStatus, evidence: Option<&Locator>) -> Result<String, AxError>;  // P4.01

pub const MEMO_OUTLINE_FIELDS: [&str; 6];    // Current goal｜Current stage｜Next action｜Blocked by｜Decision index｜Checkpoint index
#[non_exhaustive] pub enum MemoShape { WellFormed, Malformed { missing: Vec<&'static str> } }
pub fn check_memo_shape(text: &str) -> MemoShape;

// P1.09（gate）：目的地半边的门，与载荷半边的 egress 并列
// pub struct EgressAllowlist; pub fn egress_target(&EgressAllowlist, &EgressTarget) -> EgressOutcome
// 域名按标签边界匹配（notexample.com 不命中 example.com）；空表＝不通公网；回环与私网不算出网
pub fn tally(rows: &[RoadmapRow]) -> Progress;   // P1.04：Progress／PlannedProgress／UnplannedProgress 携 serde
// （与 Completion 相反：进度是界面要渲染的一次读数，不是「已完成」的主张；不可反序列化的仍是 Done）   // 对账规则 1：Done 且 Present 才计入分子；Blocked/AwaitingApproval 计 blocked
#[non_exhaustive] pub enum ScopeChange { Keep, Add, Drop }        // 范围变更三动词（数据面，S3 projection 消费）
#[non_exhaustive] pub enum WriteMoment { BeforeReport, AfterFeedback, OnPlanChange }  // 落盘三时机（同上）
```

- **校形状与校证据分两道门**：`check_*` 只答写得像不像样（四列齐、状态在枚举、六字段在场）；证据真伪归对账（tally 不计无证 Done，Invalid 另标存疑——EvidenceCell 三分使两者可区分）。
- 表解析：首个四列表头块；列数不等、状态拼写外、index 非数字逐条进 problems；Memo 只验大纲六字段存在（行首命中）。纯字符串处理，无 I/O。
- **拼写单套且不区分大小写**（S5.09）：运行时文档全面英文化后，中英两套拼写会成为「一个 Resident 允许写什么」的第二个权威；而大小写不入契约是因为 `done` 这类行表达的事实表装得下，把它判成 Malformed 等于拿一个读者不接受的理由把该行逐出分母。
- **写者与读者同住（P4.01）**：`set_roadmap_status` 是这张表唯一的编辑入口。别处拼一行 Markdown 就是对「一行长什么样」的第二份意见，而语法只能有一份。三条契约：只改首个四列表（与 `check_roadmap_shape` 同一块）；输出行规范化为 `| index | item | status | evidence |`，故**同一次改写两次得到同一字节**；`Done` 缺证据恒拒（`E_EVIDENCE_MISSING`）——那正是 `tally` 拒绝计入分子的那种行，允许写出来就是允许制造一个看起来完成、数字却不动的行。

### 8-20 kernel::completion（S2.08）

```rust
pub struct Evidence(/* Vec<EventRef> 私有 */);
impl Evidence {
    /// Non-empty and every ref kind ∈ {tool_result, model_returned},
    /// else E_EVIDENCE_MISSING. A6's type half.
    pub fn new(refs: Vec<EventRef>) -> Result<Evidence, AxError>;
    pub fn refs(&self) -> &[EventRef];
}
#[non_exhaustive] pub enum Completion { Done(Evidence), Limit, Cancelled }

pub struct PlannedProgress { pub done: u32, pub blocked: u32, pub total: u32 }
impl PlannedProgress { pub fn ratio(&self) -> (u32, u32); }      // (done, total)；呈现方自算百分比
pub struct UnplannedProgress { pub steps: u32, pub budget: BudgetUse }   // 无 ratio 方法：类型层诚实（A17）
#[non_exhaustive] pub enum Progress { Planned(PlannedProgress), Unplanned(UnplannedProgress) }
```

- 两态分两 struct 而非 enum 携字段：百分比方法只能长在 Planned 上，Unplanned 拿不到——「界面拿不到百分比就画不出百分比」的类型形态。
- `EventRef` 新增 `pub fn kind(&self) -> EventKind`（Evidence 校验需读 kind；公开面变更随本 SPEC 同集）。

### 8-21 kernel::approval（S2.08）

```rust
pub struct ApprovalId(String);               // 非空；uuid v7 由效果层发，kernel 不生成
#[non_exhaustive] pub enum ApprovalSource { Gate, Agent }
#[non_exhaustive] pub enum ApprovalClass { Commitment, BudgetLimit, DiscardEscalate, AgentQuestion }
pub struct ClusterKey { pub class: ApprovalClass, pub detail: String }
pub struct ApprovalItem { pub id: ApprovalId, pub source: ApprovalSource, pub actor: String,
                          pub action_desc: String, pub artifact: Locator, pub cluster_key: ClusterKey,
                          pub created: TimeMs, pub tainted: bool }

#[non_exhaustive] pub enum PolicyClass { AgentQuestion }          // 可免审类：三必经人类无 variant 可写（类型层禁止）
pub struct PolicyMatcher { pub class: PolicyClass, pub detail_prefix: String }
#[non_exhaustive] pub enum PolicyVerdict { Allow, Deny }
pub struct Policy { pub id: String, pub matcher: PolicyMatcher, pub verdict: PolicyVerdict,
                    pub source: ApprovalId, pub created: TimeMs, pub last_hit: Option<TimeMs> }
                    // 无 revocable 字段：恒真字段不入型（false 不可表示）
#[non_exhaustive] pub enum PolicyApplication { Applies(PolicyVerdict), NotApplicable }
pub fn match_item(policy: &Policy, item: &ApprovalItem) -> PolicyApplication;   // tainted 恒 NotApplicable（C15）
#[non_exhaustive] pub enum PolicyExpiry { Active, Expired }
pub fn expiry(policy: &Policy, now: TimeMs) -> PolicyExpiry;      // idle ≥ POLICY_IDLE_DAYS → Expired；checked
#[non_exhaustive] pub enum PolicyRevocation { Revoked, Expired, Superseded }   // policy_revoked reason 数据面

#[non_exhaustive] pub enum Autonomy { Owner, Delegate(ResidentId), Deferred }
#[non_exhaustive] pub enum Answerer { Human, Resident(ResidentId) }
#[non_exhaustive] pub enum AnswerVerdict { May, HumanOnly, SelfApprovalBarred, NotTheDelegate }
pub fn may_answer(autonomy: &Autonomy, item: &ApprovalItem, answerer: &Answerer) -> AnswerVerdict;
```

- **三必经人的类型化**：`PolicyClass` 不含 Commitment/BudgetLimit/DiscardEscalate，免审规则对三类**不可表示**；`match_item` 对 `tainted` 恒 NotApplicable（C15 的 Taint 条）。
- `may_answer`：Human 恒 May；Resident r 仅当 autonomy==Delegate(r)（否则 NotTheDelegate）且 item 不属三类且 !tainted（否则 HumanOnly）且 item.actor ≠ r（否则 SelfApprovalBarred）。Deferred 下 Resident 恒 HumanOnly——没有人应答是事实的名字，不是新判定。
- verdict 先落账再生效、前拦不烧 token：效果层顺序约束（S3/S4），kernel 只出判定。
- `AUTONOMY_DEFAULT: Autonomy = Owner` 随本卡落 consts_policy。

### 8-22 kernel::config（S2.09）

```rust
#[non_exhaustive] pub enum ClockStampGranularity { Off, Minute, FiveMinute, Hour }   // 类型住 kernel 非 runtime
pub struct LayeredValue<T> { pub city: Option<T>, pub building: Option<T>, pub resident: Option<T> }
impl<T> LayeredValue<T> { pub fn resolve(&self) -> Option<&T>; }  // resident→building→city 下层覆盖上层

pub struct FrozenConfig { pub clock_stamp: ClockStampGranularity }   // Run 起点冻结；[model]/[clock] zones 字段 S3 只加
pub struct LiveConfig {}                                             // 热载面；S4 起填（PowerMode 等）
pub fn freeze(clock_stamp: &LayeredValue<ClockStampGranularity>) -> FrozenConfig;   // 缺省 CLOCK_STAMP_DEFAULT
```

- **无字段交集可机械判**：单测将两型缺省值 serde 成 JSON，断言键集交集为空；新增字段自动入判。
- `CLOCK_STAMP_DEFAULT: ClockStampGranularity = Off` 随本卡落 consts_policy（Handoff 坑 5 清账）。

**S3.10 增（config）**：`ClockZone { id, offset_min }`（已解析偏移，恒不记时区名——重解会随时区库版本分叉重放历史）；`FrozenConfig` 增 `clock_zones: Vec<ClockZone>`，`freeze` 增梯入参；zones 梯整表覆盖（下层写即替换上层全表）。本段属 kernel::config（§8-22），就近登记于此避免拆章。

**P1.10 增（config）**：`FrozenConfig` 增 `effort: Option<Effort>`（类型住 §8-24），`freeze` 增该梯入参，缺省 `None`＝不写该字段、由 provider 自行决定。

**P4.02 增（config）**：`SandboxLimits { shell: bool, fuel: u64, mounts: Vec<Address> }`，`FrozenConfig` 增 `sandbox` 字段，`freeze` 增该梯入参。三条口径：①**整值解析而非逐字段合并**——一层说到 sandbox 就说全部，于是欠说的层只会收窄而恒不会悄悄放开上层没提过的能力；②**主机事实不入城**（CPython 工件路径、shell 可执行文件位置走环境变量）——一座城被搬到另一台机器时不该带着这台机器的路径；③冻结的理由与工具表相同：**能改变可达范围的东西恒不在回合中变宽**，否则变宽的那一刻没有人审过。缺省 `fuel = SANDBOX_FUEL_DEFAULT`（`consts_policy`，2×10⁸），`shell = false`——shell 是唯一一条从参数读不出可达范围的臂。

**R1.13 增（config）／R1.17 改**：`McpServer { label: ServerLabel, transport: McpTransport }`，`McpTransport { Stdio { command, args }, Http { url, header } }`——**穷尽枚举而非两个裸字段**：一行既写 command 又写 url 就是一行要读者去猜的配置，故配置层当场拒。原始形（R1.13）是（`ServerLabel` 住 §8-23），`FrozenConfig` 增 `mcp: Vec<McpServer>`，`freeze` 增该梯入参，缺省空表＝这栋楼不接任何外部 server。三条口径：①**整表覆盖**，与 zones／sandbox 同一条理由——一层说到 `[[mcp]]` 就说全部，欠说的层只会收窄而恒不会悄悄接上上层没提过的服务；②**冻结的理由就是工具表本身**——外部工具在 Run 起点入 catalog，而 provider 把工具数组哈希在 system prompt 之前，Run 内变宽的工具表既自毁缓存又没有人审过；③**命令与参数是主机事实**（一个可执行文件在这台机器上的位置），故它们住 `CONFIG.toml` 而恒不入 Ledger 载荷——一座城被搬到另一台机器时不该带着这台机器的路径。

- **为何必须冻结**：provider 官方文档记明「switching thinking modes, changing the effort value, and changing `budget_tokens` all invalidate message cache breakpoints」——强度是缓存前缀的一部分。Run 内可变的强度＝Run 内自毁的缓存，故它落 `FrozenConfig` 而非 `LiveConfig`；设置面改它对**下一个 Run** 生效。这是那句「`[model]` 字段 S3 只加」预留位置的第一个真实居民。
- `None` 与 `Some(Effort::Off)` 是两件事：前者不写字段（provider 缺省，Anthropic 新模型即 adaptive thinking），后者显式关闭思考。不用 `Effort::Off` 兼任「未声明」，否则「没设过」与「设成关」在类型上不可分辨。

### 8-23 kernel::tool（S2.09；缝清单文件）

```rust
pub struct ToolName(String);        // 非空；ascii 小写/数字/下划线（进 catalog 与事件的名）
pub struct ServerLabel(String);     // 非空；ascii 小写/数字，恒不含下划线（R1.13；见下）
pub struct TimeoutMs(u64);          // 声明即承诺可协作取消
#[non_exhaustive] pub enum Effect { Read, Write { domain: Address }, Egress,
                                    Connector { label: ServerLabel }, Spend }   // 决定过哪道门
#[non_exhaustive] pub enum Temporal { Timeless, Timestamped }
#[non_exhaustive] pub enum CostTier { Free, Light, Heavy }        // 三档起步，对扩展开放；路由/预算消费在 S3
#[non_exhaustive] pub enum RenderIntent { Generic, Terminal, Diff { locations: Vec<Address> } }
                                    // meta 级声明用空 locations；逐调用的 locations 是 args 的纯函数（S3 工具侧）
pub struct ToolMeta { pub name: ToolName, pub disclosure: String, pub params: Payload,
                      pub effect: Effect, pub cost_tier: CostTier, pub timeout: Option<TimeoutMs>,
                      pub render: RenderIntent, pub temporal: Temporal }   // 八字段，缺一不可
pub struct ToolCall { pub id: String, pub name: ToolName, pub args: Payload }
                                    // id：S3.08 增——tool_use↔tool_result 对号是两 Dialect 的 wire 硬性要求；
                                    // 脚本适配器用确定性合成 id（call-<n>）
pub struct ToolOutcome { pub result: Payload }

pub trait Tool {
    fn meta(&self) -> &ToolMeta;
    /// Fail-closed identity: a call whose name differs from meta().name
    /// must return E_INVALID_ARGS, never route silently.
    fn invoke(&mut self, call: &ToolCall) -> Result<ToolOutcome, AxError>;
}
#[cfg(feature = "conformance")]
pub fn assert_tool_conformance<T: Tool>(tool: &mut T);   // 八字段完备＋name 文法＋错名调用拒收

pub enum ExecArm { Program { path: String, args: Vec<String> }, Python { code: String }, Shell { text: String } }
                                    // 三臂恒三（L0 冻结面），故穷尽不标 non_exhaustive；discard::forecast 的入参
```

- **`params` 复用 `Payload`**：键序 BTreeMap＋拒浮点白拿；schema 约定属 S3 工具实珰。
- **conformance 三断言**：①meta 八字段形状合法（name 文法、disclosure 非空）；②错名调用拒收（E_INVALID_ARGS）；③拒收后工具仍可用（再次正确调用不受污染）。
- ExecArm 住本模块而非 runtime：discard::forecast（S2）先于 exec 工具（S3）需要它；工具面参数枚举属 tool 面（「可枚举的必用枚举」）。
- **`Effect::Connector { label }`【R1.13 新增】**：目的地由**登记**而非逐调用参数定的那一类出站。`Egress` 的主语是一次调用（去哪台主机写在 args 里），`Connector` 的主语是一件工具（它恒只通往那一台 server）。**两者不得合并**：合并后要么让模型去填一个城自己已经知道的 `host`（一个可以填错的事实），要么让出站门拿不到目标而无法判定。发现它的时刻就是接线的时刻：P4.06 写下 `Effect::Egress` 时没有调用方，而第一次真调用当场拿到 `E_INVALID_ARGS: declares Egress but named no host`。
- **`ServerLabel` 住本模块而非 protocol【R1.13 迁入】**：它是一台 MCP server 在城里的名字，也是它每件工具名的第一段（`{label}_{tool}`），故它的文法就是 `ToolName` 的文法减下划线——写在两个 crate 里就是一条规则两个权威。**减下划线是判定而非口味**：允许它会让 `apps_foo_bar` 同时读作两种拆法，而这个名字要路由一次调用。迁入后 `city::config_layers` 在文件边界就能解析它（city 只见 kernel），于是「非法标签」在 Run 存在之前就不可表示。

### 8-24 kernel::model（S2.09；缝清单文件）

```rust
#[non_exhaustive]
pub struct BuildingPolicy { pub confidential: bool }      // S2 最小；构造子 new(confidential)，字段 S3+ 只加
pub struct ModelRequest { pub policy: BuildingPolicy, pub segments: [B3Hash; 4] }
                                    // segments＝冻结 prefix 分段哈希（与 prompt_assembled 同源）；线格式字段 S3 只加
pub struct ModelReturn { pub message: Payload, pub calls: Vec<ToolCall> }
                                    // message＝助手内容（入窗载荷）；calls＝请求的工具波（空＝本回合无工具，回合层据此收束）
pub trait Model {
    /// One provider call; adapters never sample clocks or read globals.
    fn call(&mut self, req: &ModelRequest) -> Result<ModelReturn, AxError>;
}
#[cfg(feature = "conformance")]
pub fn assert_model_conformance<M: Model>(model: &mut M);
```

**S3.01 增：canonical 会话类型族**（城内规范 Dialect 的缝上定义；gateway::dialect 只做翻译，两适配器与剧本模型消费同一形）：

```rust
#[non_exhaustive] pub enum Role { User, Assistant }                      // wire 枚举，开放
#[non_exhaustive] pub enum StopReason { EndTurn, ToolUse, MaxTokens }
pub struct SystemBlock { pub text: String, pub cache: bool }             // cache＝显式断点标记
#[non_exhaustive] pub enum ContentBlock { Text{text} | Thinking{thinking, signature}
                                        | RedactedThinking{data}
                                        | ToolUse{id, name: ToolName, input: Payload}
                                        | ToolResult{tool_use_id, content, is_error} }
pub struct ChatMessage { pub role: Role, pub content: Vec<ContentBlock> }
pub struct ToolDef { pub name: ToolName, pub description: String, pub input_schema: Payload }
pub struct ChatRequest { pub model: String, pub max_tokens: u64, pub system: Vec<SystemBlock>,
                         pub messages: Vec<ChatMessage>, pub tools: Vec<ToolDef> }
pub struct ModelUsage { pub input_tokens: Tokens, pub output_tokens: Tokens,
                        pub cache_read_tokens: Tokens, pub cache_write_tokens: Tokens }
pub struct ChatResponse { pub content: Vec<ContentBlock>, pub stop: StopReason, pub usage: ModelUsage }
pub fn message_payload(content: &[ContentBlock]) -> Result<Payload, AxError>;  // model_returned 载荷的唯一成形处
pub fn value_has_float(value: &serde_json::Value) -> bool;               // wire 面浮点禁令的判定原语
```

- 工具入参／schema 用 Payload：浮点禁令在缝上即成立（这些字节逐字进 Ledger 载荷）；provider 送浮点工具入参＝E_WIRE_MISMATCH（fail-closed，城规优先）。
- ModelRequest 增 chat 字段已随 S3.08 落地（turn 的 Assembling 相组 ChatRequest 入请求）；ModelReturn 增 usage/stop/billed 三字段已随 S3.02 落地，携 `bare()`（脚本最小构造）与 `from_response(resp, billed)`（tool_use 块→波，全量入账）两构造面；turn 的 model_returned 载荷随之增 usage／stop／billed_usd_micros（在场才写）。
- BuildingPolicy 住本缝而非 city：kernel 不能依赖外层，city::policy（P1）是它的**求值器**不是定义处（依赖反转，同 ledger 缝）。
- conformance 两断言：①良性请求得 Ok 且 message/calls 形状合法（类型已保大半）；②Err 后适配器不中毒（再调仍得应答）。确定性不入 conformance（真 model 非确定），剑本适配器的确定性由 citysim 自证。

**P1.10 增：思考记录与思考强度**（思考记录原样保留，消息往返恒按 provider 官方规定处理）

```rust
#[non_exhaustive] pub enum Effort { None, Low, Medium, High, XHigh, Max }   // 全序；Ord 按声明序
pub fn content_from_message(message: &Payload) -> Result<Vec<ContentBlock>, AxError>;  // 契约变更，见下
pub struct ChatRequest { /* …既有五字段… */ pub effort: Option<Effort> }
```

- **两个思考块，逐字保留**。provider 官方规定：「During tool use, you must pass thinking blocks back to the API for the last assistant message. Include the complete unmodified block back」；改动即 400 `invalid_request_error`，报文为「`thinking` or `redacted_thinking` blocks in the latest assistant message cannot be modified」。故 canonical 侧两个变体缺一不可，字段名与线上同名（`thinking`／`signature`／`data`），使翻译无重命名、使 Ledger 载荷可直接对照官方文档校读。`signature` 是「an encrypted copy of the full reasoning」，由 provider 验签，城内恒不解析、不截断、不重排。
- **为何不是「可选保留」**：两条独立理由各自足以定案。其一，丢弃即违约（上一条）。其二，`message_payload` 是 `model_returned` 载荷的唯一成形处，脱机重建窗口靠它；入账前剥掉思考块，重建出的窗口就是一个从未发送过的窗口——那是判负条件三（历史失真），而它一旦成立，本设计的一切保证同时作废。**用户给出的第三条理由是缓存**：改写助手消息即换缓存前缀。
- **`content_from_message` 契约收紧**（本卡修既有缺陷）：`content` 键缺席仍折为空块表（脚本载荷的既定行为）；`content` 键在场却解不出，此前经 `.ok().unwrap_or_default()` 静默折为空——一个未知块类型会让整条助手消息在窗口里消失，而 Ledger 里它还在。两条历史就是这么长出来的。改为在场即必须解出，否则 `E_WIRE_MISMATCH`。返回类型随之由 `Vec<ContentBlock>` 变 `Result<Vec<ContentBlock>, AxError>`。
- **`Effort` 六级**：两家实际在用的就是 `none/low/medium/high/xhigh/max`，不另列其他方案。一处差别写清楚：**Anthropic 的 `effort` 只收五级**（官方 SDK 类型 `Literal["low","medium","high","xhigh","max"]`），`none` 不是它的取值，关思考在另一个字段 `thinking:{type:"disabled"}`；官方另记「Setting `effort` to `"high"` produces exactly the same behavior as omitting the `effort` parameter entirely」。OpenAI 侧六级同名（其 `minimal` 属 gpt-5 旧拼写，不入城内梯子）。故**两方言都拼得出全部六级**，早先设想的「方言拼不出就拒」路径作废；dialect 里只留 fail-closed 通配臂，含义改为「日后新增的级别尚未教会写」，恒不夹取到邻级。
- **不建每模型强度支持表**：任何 provider API 都不返回「本模型支持哪几级」。造一张我们填不满的表，就是给 provider 的真实行为立第二个权威；模型自己拒的原样透出。
- **`max_tokens` 是模型的事实，不是调用方的偏好**：Anthropic 要求每请求必带 `max_tokens`，且开思考时它是「思考＋回答」的总上限；OpenAI 则可缺席。两家的 `GET /v1/models` 都不返回该上限，所以它探不到，只能随模型登记。权威定在 `gateway::market::ModelEntry.max_output_tokens`，`CallShape.max_tokens` 由选型点从那一行解出；**任何调用处手写数字即错**——截断会发生在一个账上找不到理由的地方。

### 8-25 kernel::secret（S2.10）

```rust
pub struct SecretRef { /* realm, name —— 私有 */ }
impl SecretRef { pub fn parse(raw: &str) -> Result<Self, AxError>;   // secret:<realm>/<name>；形状非法＝E_CONFIG_INVALID
                 pub fn realm(&self) -> &str;  pub fn name(&self) -> &str; }
// Display "secret:<realm>/<name>"；serde 字符串形；恒不入 Locator 文法（两解析器分立）

pub struct SecretSpan { pub start: usize, pub len: usize, pub provider: Option<&'static str> }
                                    // provider 来自形状表命中；None＝熵侦测器命中
/// Custody's detection half: shape table first,
/// entropy second. Pure, no regex, no backtracking; kani-provable
/// termination. Replacement/vaulting is the effect layer's (S3).
pub fn scan(bytes: &[u8]) -> Vec<SecretSpan>;

pub struct Sealed<T: zeroize::Zeroize>(/* secrecy::SecretBox<T> */);
impl<T: zeroize::Zeroize> Sealed<T> {
    pub fn new(value: Box<T>) -> Sealed<T>;
    /// Call sites are whitelisted by `xtask secret` (gateway::endpoint/
    /// native only, S3); the type refuses Debug/Display/Serialize so a
    /// sealed value cannot reach any sink even by accident.
    pub fn expose(&self) -> &T;
}
```

- **扫描两侦测器**：①形状表（SECRET_SHAPES：前缀＋字符集＋长度窗）为主；②熵阈为辅——无前缀命中的 token 段（base62/base64url 字符连段，长度 ≥ `ENTROPY_SPAN_MIN_BYTES=20`，pub(crate) 内部事务）且每字符熵 ≥ `SECRET_ENTROPY_MIN`（3.5 bits/char）。两集合并，重叠段归形状命中（provider 信息更多）。
- **熵的整数化**：kernel 禁浮点——香农熵以 millibit（1/1000 bit）计：定点 log2（shift-and-square，10 位小数位，循环界常数）；判式 `mb·den ≥ num·1000`（checked）。kani：任意输入终止、无 panic、无溢出。
- **Sealed 取 secrecy::SecretBox**（secrecy 0.10.3＋zeroize 1.9.0，钉版 B.7）：drop 即零化；无 Debug/Display/Serialize/Clone；trybuild 反例＝Sealed 值入 EventRecord/format! 编译不过。`PutSecret` 的命令面（S4）直用本类型。
- 误报是既知常态（入口无损可逆，出口才拒）；`E_SECRET_EGRESS` 的 subject 恒不回显命中字节（塑形在 gate::egress）。

### 8-26 kernel::discard（S2.10）

```rust
#[non_exhaustive]
pub enum Restoration { Tracked(Locator), Interred(Locator), Rebuildable { reason: String } }
pub struct Discard { /* paths, plan, taint, total_bytes —— 私有 */ }
impl Discard {
    /// Sole constructor (C14): restoration mandatory and scheme-checked —
    /// Tracked wants file:, Interred wants cas:, Rebuildable wants a
    /// non-empty reason; violations are E_DISCARD_IRREVERSIBLE.
    pub fn new(paths: Vec<Address>, plan: Restoration, taint: TaintSet, total_bytes: ByteLen)
        -> Result<Discard, AxError>;
    pub fn paths(&self) -> &[Address];  pub fn plan(&self) -> &Restoration;
    pub fn taint(&self) -> &TaintSet;   pub fn total_bytes(&self) -> ByteLen;
}

#[non_exhaustive] pub enum DiscardRequest { Planned(Discard),
                                            Unplanned { paths: Vec<Address>, taint: TaintSet, total_bytes: ByteLen } }
#[non_exhaustive] pub enum EscalateReason { FilesOverMax, BytesOverMax, RegistryAsset, Tainted }
#[non_exhaustive] pub enum DenyReason { NoRestoration }
#[non_exhaustive] pub enum DiscardVerdict { Allow, Escalate { reason: EscalateReason }, Deny { reason: DenyReason } }
/// The fifth door's decision table, sole authority —
/// gate::discard delegates wholly and only shapes the refusal.
pub fn decide(req: &DiscardRequest, registry: &Registry) -> DiscardVerdict;

#[non_exhaustive] pub enum DiscardForecast { Clear, Suspected { pattern: String } }
pub fn forecast(arm: &ExecArm) -> DiscardForecast;
```

- **decide 表**：Unplanned → Deny{NoRestoration}（无还原不可构造使 Planned 恒有 plan，Deny 只剩这一条路）；Planned：taint 非空 → Escalate{Tainted}（恒，无视规模）；paths 数 > `DISCARD_FILES_MAX` → Escalate{FilesOverMax}；total_bytes > `DISCARD_BYTES_MAX` → Escalate{BytesOverMax}；任一 path 命中 Registry Asset（以 file: Locator 前缀形归一查 is_asset；S2 取路径字符串相等）→ Escalate{RegistryAsset}；余 Allow。判序固定（Tainted→Files→Bytes→Asset），确定可重放。
- **forecast 三臂预判力递减**：Program 读 `(path, args)` 整体——basename ∈ {rm, rmdir, del} 或 git 携 reset --hard/clean 或 find 携 -delete；Python/Shell 子串表（rm 、rmdir、-delete、git reset --hard、git clean、os.remove、shutil.rmtree、os.unlink；Shell 另含 `>` 截断重定向）——可被混淆绕过，恒保守；git 兑底在 S3 checkpoint。子串表是 pub(crate) 数据面。
- kani：Discard 门 fail-closed——Unplanned 恒不 Allow；Tainted 恒不 Allow。

### 8-27 kernel::gate（S2.04；施工序最后）

```rust
pub struct GateContext { pub actor: String, pub now: TimeMs, pub item_id: ApprovalId }   // Escalate 造 item 所需；全由调用方注入
#[non_exhaustive] pub enum GateOutcome { Allow, Escalate { item: ApprovalItem }, Deny { refusal: Box<AxError> } }

pub fn domain(domain: &WriteDomain, target: &Address, taint: &TaintSet) -> GateOutcome;
#[non_exhaustive] pub enum EgressTarget { Loopback, Private, Public { host: String },
                                          Connector { label: ServerLabel } }   // 分类由效果层解好址后注入
#[non_exhaustive] pub enum EgressOutcome { Allow { first_public_egress: bool }, Deny { refusal: Box<AxError> } }
pub fn egress(spans: &[SecretSpan], target: &EgressTarget, prior_public_egress: bool) -> EgressOutcome;
pub fn spend(ladder: &BudgetLadder, cost: &BudgetUse, taint: &TaintSet, ctx: &GateContext) -> GateOutcome;
#[non_exhaustive] pub enum CommitmentDecision { Approved, Denied }
pub fn commitment(decision: Option<&CommitmentDecision>, taint: &TaintSet, ctx: &GateContext,
                  action_desc: &str, artifact: &Locator) -> GateOutcome;
pub fn discard(req: &DiscardRequest, registry: &Registry, ctx: &GateContext,
               action_desc: &str, artifact: &Locator) -> GateOutcome;
pub fn spawn(parent: Depth, kind: &DelegateKind) -> GateOutcome;      // E_DELEGATION_DEPTH 的塑形处

#[non_exhaustive] pub enum DedupVerdict { Fresh, Duplicate }
pub fn dedup(seen: &BTreeSet<IdemKey>, key: &IdemKey) -> DedupVerdict;   // 去重恒先于副作用：调用序纪律＋citysim 不变量看守
```

- **gate 是全库唯一 gate 码生产者**：五门 Deny 恒经 `AxError::refusal`（三段必填）；Domain 门 nearby＝domain 前缀表；Discard 门 alternative 恒可执行（分批或 Interred 后重试）；Egress 门 subject 只写位置与跨度数，恒不回显命中字节。
- **Escalate 的二源归一**：spend 耗尽 → item{class: BudgetLimit}；commitment 无决 → item{class: Commitment}；discard Escalate → item{class: DiscardEscalate}；均 source=Gate、tainted＝taint 非空（C15 标记位）。commitment 携 Denied 决定 → Deny（E_APPROVAL_DENIED，非 gate 码故用 failure 形）。
- **Taint 升档的 S2 实例**：Discard 门 Tainted 恒 Escalate（住 discard::decide）＋Escalate item 的 tainted 标记位（封 Policy/代答）。其余门的升档语义随其审批面出现时实例化（P1/P2），本期不造无消费者的规则。
- **首次公网出网**：`egress` 对 Public 且 `!prior_public_egress` 置 `first_public_egress`；NetNotice 挂信封属 pipeline（S3）。Loopback/Private 恒不触发（对 localhost 提醒注入只会训练模型忽略提醒）。
- kani：五门组合 fail-closed——reserved 目标恒不 Allow；spans 非空恒 Deny；超预算恒不 Allow；Unplanned Discard 恒不 Allow；Delegated 再派生恒不 Allow。

## 8.5 两个设计（crate 级）

**A（选中）：规范字节住 kernel**——`EventRecord::canonical_line` 是全库唯一字节产地，jsonl／citysim 内存 Ledger／replay 三个消费者共用；conformance 断言 4 因此可写。杠杆：V8「三平台字节一致」收敛为一个函数的性质；换适配器不换字节。
**B（落选）：各适配器自产字节**（kernel 只给结构体，序列化归落盘方）——貌似「端口薄」，实则把规范散进每个适配器：内存 Ledger 与 jsonl 各持一份 serde 配置，漂移即 A19/A15 失真，而 conformance 只能对拍两实现、无法指认哪份是规范。落选理由：链对原始字节计算，字节即语义，语义必须一处。翻案条件：出现「同一记录合法多形」的需求（现设计明拒此需求）。

**第二对（error 侧）**：carrier 声明在 `AxCode::carrier()` 穷尽 match（选中）vs 分立静态表 `[(AxCode, Carrier); 35]`。选中方案让「新增码忘配 carrier」成为编译错误（非穷尽 match 不过编译）；静态表则要靠测试数分支。落选表的唯一优势是 specalign 好解析——但 specalign 对齐的是 SPEC 表与 enum，match 臂同样可数。

**第三对（S2，gate 侧）**：五门分立函数（选中）vs 单一 `gate::check(ActionEnvelope) -> GateOutcome` 总入口。总入口看似接口更窄，实则要造一个能同时表达五种异质入参的胖信封（写目标、出网目标、预算梯、审批决定、删除请求的交集形状），每门只读其中一角——胖信封即接口谎言，且无法逐门 kani（状态空间相乘）。五函数共享 GateOutcome 与 refusal 塑形纪律，组合在调用方（效果层按 Effect 字段选门）。落选的总入口若日后出现（如 wire 面需单帧过门），作为薄路由层另立，不回收五函数。

## 9 工作流程

写路径：调用方组 `EventDraft`（Payload 构造点已拒浮点）→ `Ledger::append`（适配器：定 seq/prev → `EventRecord::from_draft` → `canonical_line` → 落介质）→ 返回 `EventRef`。
读路径：适配器/replay 逐行 `parse_line` → 验 v/链/seq → `to_ref` 铸引用。
错误路径：一切构造与解析失败即 `AxError`（fail-closed），生产模块负责把它送到 carrier event（S2 起）。

## 10 实现逻辑

0. **AxError 内部装箱**：`{ code, Box<其余六字段> }`，serde flatten 保持 wire 形与字段序不变。理由：AxError 走每一道缝的返回位，扁平七字段 176 字节超 `result_large_err` 阈（128）；装箱后 16 字节，接口与序列化形态零变化。
1. 全模块零 I/O、零时钟、零随机；BTreeMap/BTreeSet only（本期仅 Payload 经 serde_json::Map 默认 BTreeMap 间接满足）。
2. hex 编解码手写（16 行内，查表小写），不引 hex crate——C12 精神：依赖面只进钉版清单所列。
3. `EventKind`/`AxCode` 的 serde 呈现名逐 variant `#[serde(rename = …)]`（AxCode）与 `#[serde(rename_all = "snake_case")]`（EventKind）；`as_str` 与 serde 用同一份拼写（单测对拍）。
4. `Payload` 校验递归下降 serde_json::Value：`Number::is_i64 || is_u64` 之外即拒；数组与对象深入。递归深度由输入方（我们自己的写方）有界，读侧 parse_line 对深度不设限但对浮点恒拒。
5. `canonical_line` 用 `serde_json::to_vec`；`addr`/`ig` 的省略由 `skip_serializing_if` 表达；无 pretty、无空格。

## 11 边界枚举

空 Payload（合法，`{}`）；`ig:true` 且未知 kind（读侧放行跳过——replay 章）；`Seq::MAX.next()`（E_INVALID_ARGS，实践不可达但算术必 checked）；`Range` 端点相等（合法，单行/单字节）；`B0-0`（首字节）；`L1-1`（首行）；地址单段（合法）；`RESERVED_PREFIX` 恰为全路径（is_reserved 真）；`".sprawlingx/a"`（首段非 `.sprawling`，不 reserved——段边界判定）；Locator 尾随空白（拒）；hex 奇数长（拒）；`E_...` 码字符串反序列化未知码（serde 报错→读侧 fail-closed）。

## 12 错误处理（逐码答「能否定义掉」——规则十）

本期激活的码：

- `E_INVALID_ARGS`（address/payload/seq 构造拒）：不可定义掉——解析面即 Taint 边界，输入天然不可信；类型把「构造后非法」定义掉了，「构造时非法」必须留码。
- `E_LOCATOR_INVALID`：同上；且与宽松接受严格互斥（fail-closed 是策略）。
- `E_VERSION_CONFLICT`（verdict 映射在 S3）：不可定义掉——乐观并发的存在理由就是冲突可发生。
- `E_LOG_VERSION_UNSUPPORTED`／`E_CAS_CORRUPT`：住装载期白名单，产生地在 memory/runtime（见各自 SPEC）。
- `E_STORAGE_FATAL`（存储写失败，装载期）：不可定义掉——磁盘满与介质 Io 失败在设计边界外；宁停不脏要求它直达进程级 fatal，不得伪装成可重试。S2 期初增设；memory 的 Io 映射已改正（memory-SPEC §12）。

S2 激活的码（逐码答「能否定义掉」）：

- `E_OUTSIDE_WRITE_DOMAIN`：不可——写目标是运行期输入，类型只能封构造后非法，封不住越域目标。
- `E_GATE_DENIED`：不可——Commitment 拒绝需要通用拒码；其余四门各有专码。
- `E_TAINTED_ACTION`：不可——Taint 升档后被拒的动作需要自述来路的码（生产在 P2 注入剧本接入时）。
- `E_BUDGET_EXHAUSTED`：不可——耗尽是审批不是错误，但模型需要可机读的码知道自己停在哪。
- `E_LOOP_SUSPECTED`：不可——停滞是观测事实；定义掉它等于假定模型不会循环。
- `E_GOAL_CONFLICT`／`E_REPAIR_BUSY`：不可——同资源相斥与修复串行化是机制存在理由；Queued/Conflict 是合法结局，码只在回传面携信息。
- `E_DELEGATION_DEPTH`：不消解（明裁：边界反馈优于沉默缺席）。
- `E_APPROVAL_PENDING`／`E_APPROVAL_DENIED`：不可——前拦等待与拒批都是用户可达状态。
- `E_EVIDENCE_MISSING`：部分定义掉——无证据 Done 已不可构造（类型半）；构造时拒绝仍需此码（运行时半，A6 双守）。
- `E_SECRET_EGRESS`／`E_DISCARD_IRREVERSIBLE`：不可——两门存在的理由即这两类越界可发生；类型已把「无 Restoration 的 Discard 值」定义掉，Unplanned 请求（exec 预判路）是剩余不可消部分。
- `E_CONFIG_INVALID`：不可——SecretRef 形状非法与明文入配置必须在反序列化即拒。

其余未激活码随其生产模块的 SPEC 章逐码作答（S3+）。

## 13 依赖选型

`serde`＋`serde_json`（规范字节与载荷；B.7 钉版）；`thiserror`（Display/Error derive；B.7）；`blake3`（唯一哈希，B.7 钉 S1；1.8.6 现行 stable）；`uuid`（v7 仅解析/格式化＋serde 特性，恒不启用生成特性——kernel 禁随机）。S2 增：`secrecy` 0.10.3＋`zeroize` 1.9.0（Sealed；B.7 钉 Stage 2–3，2026-08 复核为最新）。dev：`proptest`、`insta`；S2.11 增 `trybuild`。不引：hex、rand、chrono/time（时间是入参）、regex（C12：熵与形状判定手写定点算法）。

## 14 硬编码声明

- `RESERVED_PREFIX = ".sprawling"`（冻结面；改它须裁决）。
- `GENESIS_PREV = [0u8; 32]`（「创世行 prev＝64 个 0」）。
- `IDEM_DERIVE_V = 1` 与派生框架 `run(16B)||seq(8B LE)||action`（换框架＝升版本字节，旧键不撞新键）。
- Locator 文法字面（`cas:`、`file:`、`b3-`、`#L`/`#B`）：本 SPEC 的文法节即权威。
- `SECRET_SHAPES` 初版条目（公开 provider 令牌前缀，随外界增补）：`sk-ant-`（Anthropic）、`ghp_`/`gho_`/`ghs_`（GitHub）、`AKIA`（AWS AccessKeyId）、`glpat-`（GitLab）、`xoxb-`/`xoxp-`（Slack）、`AIza`（Google API key）。字符集与长度按各 provider 公开文档；条目形状见 §8-7。

Stage 2 追加：

- **`SUBAGENT_CTX_LOCK_DEFAULT = Tokens(65_536)`**（未给数值，本 SPEC 定值携证据）：主流上下文窗口 128k–200k token；Ephemeral 适用面（一次检索/摘要/跑测）按 20 回合×每回合约 3k token 上界估 60k；取 2^16 使锁高于任务上界、低于最小主流窗口之半——内耗循环在母窗口三分之一处被机械截断，正常任务不受掤。待 EVAL（P3）重估。
- `AUTONOMY_DEFAULT = Autonomy::Owner`、`CLOCK_STAMP_DEFAULT = ClockStampGranularity::Off`（直写，随类型落位）。
- 定点 log2 小数位数 10（熵判定内部事务）；`ENTROPY_SPAN_MIN_BYTES = 20`（熵侦测器最短跨度：主流 API key 最短约 20 字符；pub(crate)，改动随本 SPEC）。
- Roadmap 状态五值与 Memo 六字段的中文拼写：P2 spine_files 模板落盘时复审是否双语。

## 15 影响面

memory::jsonl／memory::cas／runtime::replay／runtime::fork／citysim 全部消费本 crate 本期公开面；S2 全部 kernel 决断模块建立在 error/event 之上。公开面变更须与本文同一变更集（S2 起 apisync 机器看守）。`consts_policy` 三项延后条目是 S2 相应卡的显式债务。

## 16 测试与约束

- 单测（各模块文件内 `#[cfg(test)]`，测试模块头挂放宽 allow）：serde 拼写对拍（as_str×serde×表）；EventKind 计数 55／in-window 计数 8（以 `ALL` 数）；carrier 全映射非重复覆盖 35；构造子不变量（refusal 三段在场、failure 无 gate、retriable 默认 false）；Payload 拒浮点（含嵌套）；Address/Locator 拒绝面正反例；Seq/Version checked 溢出；IdemKey 版本字节在场。
- proptest：`Address::parse` 往返与 `is_within` 自反/传递/反对称；`Locator` Display↔parse 往返；`IdemKey` 重算恒等＋近旁输入不等样例；`Payload` 任意整数树恒过、含浮点树恒拒。
- golden（insta）：创世行＋一条 `building_created` 的 `canonical_line` 字节。
- conformance：S1.06 对一个最小内存实现自证可跑；S1.11 citysim 实现二证。
- 约束：`cargo clippy --workspace --all-targets -- -D warnings` 零告警；无 `unsafe`；文件前三行 MPL 头。
- S2 各模块测试面（逐模块文件内 `#[cfg(test)]`＋kani 镜像 proptest）：taint 并集单调／map 保集；write_domain reserved 恒拒／夺回计数；budget 溢出＝Exhausted／逐层报首超；backpressure 单调；stall 尾部连续语义；goal/repair 重叠矩阵；delegation 静动双层；registry verify 拒非证据 kind；spine 表解析正反例＋tally 对账三情形；completion 空证据／错 kind 拒；approval 三必经人矩阵＋自审拒＋tainted 封 Policy；config 字段交集空断言；tool/model conformance 自证；secret 双语料＋熵边界；discard 决策表全分支＋forecast 三臂正反；gate 五门矩阵＋dedup＋refusal 三段非空。

## 17 模型体验

零字节：kernel 本身不进任何 Run 的 prefix。它对模型的可见面只经两物间接达成——AxError 的 three-part refusal（错误即教学，边界反馈优于开头说教）与 in-window 事件的载荷字节（由上层模块产生）。本 crate 的任何变更不影响 prefix 缓存。

## 18 文档同步

- ARCHITECTURE.md §6 kernel 表：逐卡翻状态（未建→已建）。
- 本文 §8-4／§8-1 两表是 S2 `xtask specalign` 的数据面：改 enum 必同集改表。
- `consts_policy` 三项延后：随 S2.06（锁）、S2.08（Autonomy）、S2.09（时钟档）落地，§8-8 与 §14 已登。
- 设计缺口（存储写失败码）：已消——S2 期初增 `E_STORAGE_FATAL`。
