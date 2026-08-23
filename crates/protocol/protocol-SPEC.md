# protocol-SPEC.md

> crate：`protocol`。本 SPEC 先于代码存在；实现不多不少地遵守本文。
> 骨架：apostle-sdd 十七节；按模块分章、每章自足（ARCHITECTURE.md §5）。
> 动手前先读所用工具与依赖的**官方文档或官方 agent 指南**，再写本文的接口节。

## 1 需求拆解

两件方向相反的事，各自可独立验收：

- **出站（`mcp`）**：一个 Resident 调用外部服务的工具，调用落 Ledger 且可离线重演。
- **入站（`acp`）**：一个外部编辑器把这座城当 agent 驱动，请求变成一次普通 Dispatch。

## 2 验收标准

| 单元 | 完成的定义 |
|---|---|
| mcp | 请求恒是单行且不含换行；两台 server 的同名工具恒是两个工具；浮点入参拒该次调用并报出位置；confidential 楼恒不构造该工具；录制的调用重放得同一答案 |
| acp | 已配对请求变成 Dispatch 三字段；未配对只学到一位（拒词不确认地址存在）；持有效令牌也够不到 reserved prefix；回给编辑器的只有 progress 三字段 |

## 3 假设与歧义

- **假设**：用户自己拥有外部服务的账号。本库不内置任何一家的 key、不代付、不做代理。
- **歧义已定（2026-08-22 复核）**：当前 MCP 修订版**删除了协议级 session**，`tools/list` 恒不因连接而异。因此工具表随 Run 冻结与它的规则同向，本库不实现任何会话恢复；需要跨调用状态的 server 自铸句柄，当普通入参传。

## 4 现状分析

P4 之前 `crates/protocol/src/` 只有 `lib.rs`。`kernel::tool` 缝与 taint 环已在，本 crate 只需落在它们上面。

## 5 权威信源

| 事实 | 出处 |
|---|---|
| 规范总纲与当前修订 | <https://modelcontextprotocol.io/specification/2026-07-28> |
| `tools/list` 恒不因连接而异 | <https://modelcontextprotocol.io/specification/2026-07-28/server/tools> |
| stdio 传输：子进程、按行、消息内无换行 | <https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio> |
| 删除协议级 session、新增 `server/discover` | <https://modelcontextprotocol.io/specification/2026-07-28/changelog> |

## 6 命名统一

`Connector` 是词汇表里这层的统称；代码里出现的是它的两个具体面 `McpTool` 与 `Incoming`。**恒不**把 MCP server 叫作 endpoint——`Endpoint` 在本库专指 external provider 网关。

**R1.13 改**：`ServerLabel` 迁出本 crate，住 `kernel::tool`（理由与文法见 kernel-SPEC §8-23）。本 crate 继续用它，但不再拥有它：配置层要在**文件边界**解析标签，而 `city` 只见 `kernel`。

## 7 模块边界

- **字节怎么走**归 `bin::assembly`：stdio 子进程的拉起、超时与回收住装配层；本 crate 只出一行、收一行。
- **准不准出网**归 `kernel::gate` 的 egress 门：外部工具声明 `Effect::Egress`，路由到那道门；本 crate 只在 confidential 一位上做构造点拒（更早、更硬）。
- **回来的东西算什么**归 `kernel::taint`：与 L0 工具同落 `kernel::tool` 缝，故自动进污染环，本 crate 无解包面。

## 8 接口先行

```rust
// 8-1 mcp（形状 3 端口＋形状 4 适配器＋形状 1 判定）
// R1.13 改：call 携期限。声明即承诺可协作取消（kernel-SPEC §8-23 的 TimeoutMs），
// 而一个不回答的 server 是把整个 Run 挂死的最短路径。
// P5.01 增 `notify`：一条通知没有答案。HTTP 上它被 202 加空体应答，
// 把它当请求读的客户端会因为对侧「什么都没说」而拒掉一台正确的 server。
pub trait Outbound {
    fn call(&mut self, line: &str, patience: TimeoutMs) -> Result<String, AxError>;
    fn notify(&mut self, line: &str, patience: TimeoutMs) -> Result<(), AxError>;
}
pub const PROTOCOL_VERSION: &str = "2025-06-18";
pub struct Handshake { pub protocol_version: String, pub server: String }
pub fn handshake(out: &mut dyn Outbound, rpc: &mut Rpc, patience: TimeoutMs)
    -> Result<Handshake, AxError>;
pub fn digits_for_floats(value: Value) -> Value;
// ServerLabel 住 kernel::tool（R1.13 迁出，见 §6）；本 crate 不再转导它，
// 一个类型两条导入路径就是一个类型两个住址。
pub struct Rpc { /* next: u64 私有 */ }
impl Rpc {
    pub fn initialize(&mut self) -> String;      // P5.01：取代 discover
    pub fn initialized() -> String;              // 通知，无 id
    pub fn list_tools(&mut self) -> String;
    pub fn call_tool(&mut self, name: &str, arguments: &Value) -> Result<String, AxError>;
    pub fn read(line: &str) -> Result<Value, AxError>;
}
pub fn tools_from(server: &ServerLabel, result: &Value) -> Result<Vec<ToolMeta>, AxError>;
pub struct McpTool { /* 私有 */ }
impl McpTool {
    // R1.13 改：无期限的 meta 在构造点即拒——一件没有期限的出站工具就是一件可以挂死 Run 的工具。
    pub fn new(meta: ToolMeta, remote: String, outbound: Box<dyn Outbound>, confidential: bool)
        -> Result<McpTool, AxError>;
}
pub struct ScriptedOutbound { /* 私有 */ }                                          // 第二适配器

// 8-2 acp（形状 1 判定＋形状 2 值类型）
pub struct Incoming { pub token: String, pub addr: Address, pub task: String, pub goal: String }
pub enum Admitted { Dispatch { addr: Address, task: String, goal: String } }
pub fn admit(request: &Incoming, authentic: bool) -> Result<Admitted, AxError>;
pub struct Progress { pub run: String, pub turns: u32, pub finished: bool }
```

## 8-3 生命周期与会话（P5.01）

**病灶**：本 crate 开场发的是 `server/discover`——**MCP 根本没有这个方法**。一台托管 server 对这座城说的第一句话回的是 `-32601: Method not found`。旧注释里那句「there is no protocol-level session」也不成立。

按规范改正（引 2025-06-18 Transports 与 Lifecycle 两篇的规范句）：

- **开场必须是 `initialize`**，携 `protocolVersion`、`capabilities`、`clientInfo` 三项；随后必须发 `notifications/initialized`，之后才能问别的。故 `handshake()` 是这条生命周期的**唯一权威**，坐在两个传输之上——两个传输各写一遍就是两份会漂的生命周期。
- **`capabilities` 故意为空**：roots／sampling／elicitation 是 server 反过来向**我们**要的能力；声明一项本城没实现的能力，等于招来一个随后只能拒的请求。
- **会话住传输层，不住本 crate**，因为规范把它写在 Transports 而不是 Lifecycle：server **可选**在 `initialize` 应答的头里发 `Mcp-Session-Id`；一旦发了，客户端 **MUST** 在此后每一次请求带回。404 意味着 server 结束了会话，**MUST** 重开一个——故 `bin::mcp_http` 遇 404 丢掉 id 并标 `retriable`，而不是拿一个已死的 id 永远碰下去。
- **已知的向前变化**：更新的修订正在把会话去掉（SEP-2575）。本客户端协商的是 `2025-06-18` 并按那一版行事；一台忽略该头的 server 不会因此变得不可用。
- **实测红过又绿的一条**：一台在 CDN 后面的托管 server 对**不报名的客户端**回 403 `browser_signature_banned`，早于任何 MCP 消息。故 HTTP 传输带自己的 User-Agent。

### 入向浮点：治我们发的，适应我们收的

`digits_for_floats` 把对侧答案里的小数**原样写成字符串**。真机证据：第一台被接上的搜索 server 连续四次调用全部死在一个相关度分 `1249.4` 上，模型随后开始乱抓工具。三条理由：① 禁浮点是 **Ledger 的**规矩（确定性第 6 条），不得放松；② 拒掉整个答案等于声明本城接不了任何真实的搜索 server；③ 丢掉该字段是隐形地删别人的数据。写成字符串**不丢一位数字、不做任何算术**，且在 Ledger 里看得见（带引号的数）。与 `call_tool` 拒掉携浮点的**入参**并不矛盾：本城治自己发出去的，适应自己收回来的。

## 8.5 两个设计

**第一对（浮点入参怎么处置）**：拒掉整个工具（落选）vs 拒掉这一次调用（选中）。禁浮点的真实来源是 Ledger 载荷——一次记不下来的调用就是一次重演不出来的调用。而工具的 schema 里有一个数值字段，并不说明这个工具不可用；拿一次坏参数把整件能力下架，惩罚的是下一次本来正确的调用。故按**调用**拒，并在拒词里报出那个值的路径（`a.b[1]`），让模型改的是那一处而不是猜整张表。

**第二对（confidential 怎么落）**：调用时由 egress 门拒（落选）vs 构造时就不存在（选中）。前者依赖每条路径都记得问那道门；后者让「这栋楼有一个出站工具」这件事本身不成立——那时还没有任何东西可泄。两者不冲突：egress 门仍在，这只是把同一条判断挪到更早、更便宜、更难绕的位置。

## 9 工作流程

**出站**：装配层拉起 server 子进程 → `Rpc::discover` → `Rpc::list_tools` → `tools_from` → catalog 与 bench 各注册一次（工具表随 Run 冻结）→ 模型调用 → `McpTool::invoke` → `call_tool`（浮点检查）→ `Outbound::call` → `Rpc::read` → `ToolOutcome`（污染态）→ 装配层落 `tool_called`／`tool_result`。

**入站**：装配层收 HTTP／stdio 请求 → `Incoming::parse` → 与本机配对令牌常数时间比对（`channels::auth`）→ `admit` → `Admitted::Dispatch` → 走与人相同的 `Command::Dispatch` 路径 → 期间回 `Progress`。

## 10 实现逻辑

1. **请求行手工拼装**：`format!` 而不是 `to_string(&map)`，因为 stdio 传输按行分隔且消息内恒不得含换行，而序列化器的换行策略不是本库的契约。
2. **id 由 `Rpc` 铸**：重放按「方法＋参数」建索引、恒不看 id——重放会重新编号，把 id 计入键就等于永远匹配不上。
3. **工具名加服务器前缀**：`{label}_{sanitised}`。两台 server 都提供 `search` 时，不加前缀就会有一个工具在不同的楼里做不同的事。
4. **外部工具声明 `Effect::Egress`＋`Temporal::Timestamped`**：前者把它路由到能说不的那道门，后者是事实——对侧是活的服务，答案是关于此刻的。
5. **入站拒词只泄一位**：未配对的拒绝不提地址、不提楼、不提令牌像不像。

## 11 边界枚举

非 JSON 行／非对象／既无 result 又无 error／`tools` 缺失／工具无名／标签非法／浮点在顶层、数组内、深层对象内／confidential 楼／空 token／空 task／空 goal／地址落 reserved prefix／重放缺答案。

## 12 错误处理

| 码 | 何时 | 能否让它不可能发生 |
|---|---|---|
| `E_INVALID_ARGS` | 浮点入参、入站字段缺失、路由错工具 | 部分能：浮点由模型给出，故在出口拒并指位置 |
| `E_WIRE_MISMATCH` | 答案或列表形状读不出 | 不能：对侧版本不由本库决定，fail closed |
| `E_TOOL_UNAVAILABLE` | server 返回 error、重放缺答案 | 不能：外部世界的事实 |
| `E_TIMEOUT` | 期限内未答（R1.13） | 不能：对侧多久回答不由本库决定；拒词同时是子进程被回收的那一刻 |
| `E_GATE_DENIED` | confidential 楼构造出站工具 | **能**：构造点即拒，于是「它存在过」这件事不成立 |
| `E_OUTSIDE_WRITE_DOMAIN` | 入站地址落 reserved prefix | 能：判定在 `admit`，无第二条入口 |

## 13 依赖选型

`kernel`＋`serde_json`。**恒不引入** HTTP 客户端、异步运行时、任何一家服务商的 SDK：前两者归装配层，第三者会把「本体不认识任何一家」这条承诺作废。

## 14 硬编码声明

外部工具的 `TimeoutMs(60_000)` 与 `CostTier::Heavy`：外部服务比本地工具慢一个量级，且计费。改动即改变调度与预算行为，属 15.2 行为变更。

## 15 影响面

新增 crate，无既有调用方。装配层接线时波及：子进程管理、catalog／bench 注册、`city::policy` 的 confidential 与 egress 允许表、`channels::auth` 的配对比对。

## 16 测试与约束

逐模块 `#[cfg(test)]`；「单行无换行」「同名不合并」「浮点按位置拒」「confidential 构造即拒」「未配对只泄一位」「重放同答案」六条各有一条断言。**约束**：本 crate 恒不出现 `async`、恒不持文件句柄、恒不内置任何服务商名字。

## 17 模型体验

工具名恒是 `{server}_{action}`，模型一眼看得出它在跟谁说话。浮点拒词报出 JSON 路径而不是「参数非法」，因为模型下一步要改的是那一个字段。

## 18 文档同步

`ARCHITECTURE.md` §3 缝清单（`Outbound` 一行）与 §6 protocol 两行｜`docs/third-party.md` §二（服务外挂的四条边界）｜装配层接线时同步 §6 末接线台账。
