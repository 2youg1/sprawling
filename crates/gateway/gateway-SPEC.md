# gateway-SPEC.md

> crate：`gateway`（lib，依赖 kernel）。本 SPEC 先于代码存在（十七节）。
> Stage 3 八模块：dialect／endpoint／native／credential（内缝 Vault）／oauth_profiles／admission／market／cost；router 属 P1 不在本版。
> 本 crate 覆盖的语义：模型路由；provider 客户端自写＋认证两半；Custody；市场快照与成本；model 缝；provider 侧准入。

## 1 需求拆解

| 卡 | 模块 | 一句话 |
|---|---|---|
| S3.01 | `dialect` | 城内规范 Anthropic Messages 与 OpenAI Chat 的双向纯函数翻译；保断点位／工具形状／usage |
| S3.02 | `endpoint`＋`native` | 自写线格式 HTTP 客户端（reqwest blocking＋rustls）实现 `kernel::Model`；逐字段请求覆盖；native＝回环 OpenAI 兼容服务的固定形 |
| S3.03 | `credential`＋`oauth_profiles` | Custody 效果半（scan 命中→入 Vault→原位替换 SecretRef）；兑付（组请求末格 expose，credential_lent）；describe；持久性探测；OAuth 流程（代码）＋情报表（数据） |
| S3.04 | `admission`＋`market`＋`cost` | provider 侧并发上限＋确定性最小发起间隔；模型目录快照＋钉版回滚；per-call 入账（权威计费额优先） |

## 2 验收标准

- dialect：golden（两 Dialect 各一请求一响应）＋proptest 往返（响应侧 wire→canonical→重渲染 wire 逐字段等值；请求侧断点位／工具形状／文本字节无失）。
- endpoint：对回环假 provider 服务的半流中断（SSE 截断→E_PROVIDER 且不产伪 ModelReturn）＋幂等重试（同 IdemKey 重发，对外恰一次效果——由调用方 dedup 看守，endpoint 自身无重试暗策略）。
- credential：A13 链——外来字节命中 shape→`secret_captured`（无明文无哈希前缀）→配置只见 `secret:`→resolve 兑付产 `credential_lent`→`describe` 恒不返回值。探测三态（跨重启／仅本次开机／仅本进程）可注入验证。
- admission：同一到达序列两次求值，放行时刻序逐条相同（无时钟采样，`now` 入参）。
- cost：权威计费额在场则恒胜价目推算；两源不一致时以权威为准并记差额；A20 的取材面（对账断言住 memory::attribution）。

## 3 假设与歧义

- **native 的 S3 形**＝指向回环地址的 OpenAI 兼容服务（llama.cpp/ollama 一类）的固定客户端：无凭证要求、禁非回环 base_url。本地引擎进程管理不属本 crate（P4 产品化再议）。
- **tokio 不在本期引入**：turn 是同步函数面，endpoint 用 `reqwest::blocking`（内部自管运行时线程，不出接口）。B.7 的 tokio 行推迟到 S4 channels（首个真异步消费者），偏离已记（§13）。
- 线格式 JSON 允许浮点（temperature 等 provider 字段）：dialect 是翻译面不是判定路径；判定路径（cost／admission／market 价目）恒整数。
- OAuth 活体流程不可在 CI 验证：流程状态机与请求构造以形状测试看守，端到端属人工清单。

## 4 现状分析

空壳 lib。无既有公开面（api-baseline 自本期起算）。

## 5 权威信源

自写客户端的四条被逼理由与「流程是代码、情报是数据」；Custody 全节；model 缝（native／endpoint 两生产适配器）；Anthropic Messages API 与 OpenAI Chat Completions API 官方文档（线格式字段名以官方为准）；keyring crate 文档（平台凭证服务绑定）。

## 6 命名统一

Dialect／Endpoint／Custody／Vault／SecretRef／Sealed／admission／market snapshot／UsdMicros／权威计费额（authoritative billed amount）。概念名英文原词；「兑付」＝resolve+expose 的合称。

## 7 模块边界

```
dialect（纯函数）◀── endpoint／native（I/O 适配器，impl kernel::Model）
credential ──▶ 内缝 Vault（pub(crate) trait：keyring 生产适配器＋会话内存第二适配器）
oauth_profiles（数据面，零分支）◀── credential（流程消费情报）
admission／market／cost：纯判定与数据面，被 endpoint 与 S3 回合层消费
```

**P1.10 增（market）**：`ModelEntry` 增 `max_output_tokens`——一次回答能吐多少字是**模型的事实**，不是调用处的选择；探测接口不返回它，故它随模型登记入目录行。尚未登记的本地模型沿用 `local` 行的保守上限，登记面（P1.11）接管后改为人确认过的行。

**P1.11 增（router）**：本 crate 现在持有 Endpoint 簿——它是**值不是库**，从 Ledger 重建（同 `kernel::registry` 的口径）；本 crate 仍不持 Ledger 句柄，写入由装配层做。

**不做什么**：不做 duty pool 与三轴定档（多 Agent 功能未成形之前，职责池是给一个还不存在的消费者建权威；现为标签选择，见 §8-9）；不持有 Ledger 句柄（事件载荷由调用方入账，本 crate 只产载荷值）；不缓存已解封凭证（每次操作解析一次）；不实现通用 provider 抽象层（被明拒）；dialect 不开缝（纯函数不 trait 化）。

## 8 接口先行（按模块分章）

### 8-1 gateway::dialect（S3.01；形状 1 判定函数族）

城内规范会话类型住 `kernel::model`（缝上类型，S3 只加）；本模块只做 canonical↔wire 翻译，纯函数、无 I/O、无状态。

```rust
#[non_exhaustive] pub enum DialectKind { Anthropic, OpenAi }
pub fn request_wire(kind: DialectKind, req: &ChatRequest) -> Result<serde_json::Value, AxError>;
pub fn response_from_wire(kind: DialectKind, wire: &serde_json::Value) -> Result<ChatResponse, AxError>;
pub fn response_wire(kind: DialectKind, resp: &ChatResponse) -> Result<serde_json::Value, AxError>;
                                    // 响应侧双向：往返性质可测（wire→canonical→wire 等值）；重放剧本也要它造假响应
```

- **保三样**：①断点位——canonical 的 `cache: true` 标记翻到 Anthropic 侧＝`cache_control{type:"ephemeral"}`，逐块原位；OpenAI 侧无显式断点（供应商缓存是隐式前缀匹配），翻译**记录性丢弃**（文档声明，不静默）；②工具形状——`ToolDef{name, description, input_schema}`↔Anthropic `tools[]`／OpenAI `tools[{type:"function",function:{…}}]`，逐字段；tool_use↔tool_calls（id/name/args 无失）；③usage——Anthropic `usage{input_tokens,output_tokens,cache_read_input_tokens,cache_creation_input_tokens}`／OpenAI `usage{prompt_tokens,completion_tokens,prompt_tokens_details.cached_tokens}`→`ModelUsage` 四整数字段，缺失字段取 0。
- 未知 wire 字段：请求侧不产（我们只写自己声明的字段＋overrides）；响应侧忽略未知键、缺必需键报 `E_WIRE_MISMATCH`（subject 写键路径）。
- S3.01 落地记录：canonical 枚举（Role／StopReason／ContentBlock）在 crate 外属 non_exhaustive，本模块通配臂恒 fail-closed（未知变体＝E_WIRE_MISMATCH，不猜不默）；wire JSON 键序＝serde_json BTreeMap 字典序（确定性，对端语义无关）。
- `E_ENDPOINT_DIALECT_UNSUPPORTED`：DialectKind 之外的方言请求（wire 探查失败）；本模块两码之外不新增。

**P1.10 增：思考块与思考强度的两侧翻译**

保的第四样：**思考块。**Anthropic 侧两向逐字，`thinking`（携 `signature`）与 `redacted_thinking`（携 `data`）各自原位往返；canonical→wire→canonical 与 wire→canonical→wire 两条往返均逐字节相等。理由是 provider 官方规定而非我们的偏好：改动即 400，报文指名这两个块 cannot be modified（kernel-SPEC §8-24 引原文）。

OpenAI 侧无对应物：出向翻译**记录性丢弃**思考块（同断点位的现行口径：文档声明，不静默），因为 Chat Completions 拼不出该形状且它也不要求回传；canonical 记录不受影响，重放仍能从 canonical 重推出当时实发字节（dialect 是纯函数）。入向则相反：OpenAI 回的推理摘要不伪造成 `Thinking`（它没有可回传的 signature）。

强度映射表（`ChatRequest.effort`，缺席即不写字段）：

| `Effort` | Anthropic wire | OpenAI wire |
|---|---|---|
| 缺席（`Option::None`） | 不写（等价于 `high`，官方明言） | 不写 |
| `None` | `thinking:{type:"disabled"}` | `reasoning:{effort:"none"}` |
| `Low`／`Medium`／`High`／`XHigh`／`Max` | `effort:"…"` | `reasoning:{effort:"…"}` |

两方言都拼得出全部六级，**唯一差别是「不思考」写在哪个字段**：Anthropic 的 `effort` 只收五级，无 `none`。通配臂仍 fail-closed，但它现在只能被「日后新增且尚未教会写的级别」触发（同 `role_str`／`stop_str` 的既有习惯）。

**缓存后果写在这里，因为它是选型理由**：官方排错文档记明「switching thinking modes, changing the effort value, and changing `budget_tokens` all invalidate message cache breakpoints」。故强度住 `FrozenConfig`（kernel-SPEC §8-22），Run 内不可变；本模块只负责把已冻结的值翻上线。

### 8-2 gateway::endpoint（S3.02；形状 4 适配器）

```rust
pub struct EndpointConfig {
    pub base_url: String,                       // 恒 https 或回环 http；尾斜线归一
    pub dialect: DialectKind,
    pub model: String,                          // provider 侧模型名
    pub max_tokens: u64,
    pub auth: AuthSpec,                         // 认证头模板；SecretRef 兑付在组请求末格
    pub extra_headers: Vec<(String, String)>,   // 明文头（非凭证）；凭证恒走 auth
    pub overrides: Vec<(String, serde_json::Value)>,  // 逐字段请求覆盖：JSON Pointer→值，最后应用
    pub timeout_ms: u64,
}
#[non_exhaustive] pub enum AuthSpec { Bearer(SecretRef), Header { name: String, value: SecretRef }, None }
pub struct Endpoint { /* config、reqwest::blocking::Client、resolver: Box<dyn Fn(&SecretRef)->Result<Sealed<String>,AxError>> —— 私有 */ }
impl Endpoint { pub fn new(config: EndpointConfig, resolver: /* 兑付闭包，由 credential 提供 */) -> Result<Endpoint, AxError>; }
impl kernel::Model for Endpoint { /* call：ChatRequest（req.chat）→dialect→HTTP→ChatResponse→ModelReturn */ }
```

- **组请求五步**：canonical→`request_wire`→逐条应用 overrides（JSON Pointer，后者胜）→认证头兑付（`resolver` 取 `Sealed`，`expose()` 只在写头那一格，写完即 drop 零化）→POST。响应四步：状态码判定（429/5xx→E_PROVIDER 携 retry 语义；4xx→E_PROVIDER 携 provider 错误体摘要）→`response_from_wire`→usage 抽取→`ModelReturn`。
- **半流中断**：SSE 流截断（连接断／不完整事件）＝`E_PROVIDER`，恒不产部分 ModelReturn；S3 先落非流式全量路径，流式属只加（接口不变，config 增 `stream: bool` 字段即可）。
- **无暗重试**：重试／转移是 watchdog 与 admission 的决策，endpoint 一次调用恰一次 HTTP 往返；幂等由调用方 IdemKey dedup 看守。
- S3.02 落地记录：base_url＝完整端点 URL（逐字段哲学，不拼路径）；EndpointConfig 增 pricing: Option<ModelEntry>（结算在适配器内以便 ModelReturn 携 billed 入账；权威额线上无标准槽位，现行恒 PriceSheet 源）；reqwest 0.13 的 rustls feature 名＝`rustls`（非 0.12 的 rustls-tls）；非流式先行，半流中断以截断 body 实测（E_PROVIDER，恒不产部分 ModelReturn）；kernel::ModelReturn 增 usage/stop/billed 三字段＋bare()/from_response() 两构造面（kernel-SPEC §8-24 同集）。
- `.expose(` 白名单（xtask secret）：本文件与 native.rs 是 gateway 侧仅有的两个合法出现点。

### 8-3 gateway::native（S3.02；形状 4 适配器）

```rust
pub struct NativeConfig { pub base_url: String /* 恒回环 */, pub model: String, pub max_tokens: u64, pub timeout_ms: u64 }
pub struct Native { /* Endpoint 复用 —— 私有 */ }
impl Native { pub fn new(config: NativeConfig) -> Result<Native, AxError>; }   // 非回环 base_url→E_CONFIG_INVALID
impl kernel::Model for Native { /* 委托内部 Endpoint（OpenAi dialect、AuthSpec::None） */ }
```

- 本地推理恒走此路：S3 形＝回环 OpenAI 兼容服务的固定客户端；「恒回环」由构造子强制（fail-closed），出网面因此在类型上不存在。
- 不是 pass-through 豁免：Native 的策略＝回环强制＋无凭证＋方言固定，三条都是 Endpoint 不持有的判定。

### 8-4 gateway::credential（S3.03；形状 4＋内缝 Vault）

```rust
pub(crate) trait Vault {                        // 内缝：两句话接口
    fn put(&mut self, reference: &SecretRef, value: Sealed<String>) -> Result<(), AxError>;
    fn get(&self, reference: &SecretRef) -> Result<Option<Sealed<String>>, AxError>;
    fn delete(&mut self, reference: &SecretRef) -> Result<(), AxError>;   // 探针与轮换用；不出对外接口
}
#[non_exhaustive] pub enum Persistence { AcrossReboots, ThisBoot, ThisProcess }
pub struct Described { pub configured: bool, pub source: String, pub persistence: Persistence, pub writable: bool }

pub struct Custodian { /* backend: Box<dyn Vault>、source 名、persistence —— 私有 */ }
impl Custodian {
    /// Startup probe: write-read-delete on each candidate backend, first
    /// pass wins; all failed -> session-memory fallback + provider_degraded
    /// payload returned to the caller for ledger append.
    pub fn probe() -> (Custodian, Option<Payload>);
    /// Custody effect half: spans from kernel::secret::scan, plaintext into
    /// the vault, `secret:` literals back in place. Payloads carry
    /// realm/name/origin/span length only - no plaintext, no hash prefix.
    pub fn capture(&mut self, bytes: &[u8], origin: &str) -> Result<Captured, AxError>;
    pub fn set(&mut self, reference: &SecretRef, value: Zeroizing<String>) -> Result<(), AxError>;
                                    // 遮蔽即拒；空值即未配置；入参取 Zeroizing 非 Sealed（S3.03 裁定）：
                                    // `.expose(` 白名单恒三文件（定义处＋两解封点），Custody 是库不是 sink——
                                    // 持 Sealed 者恒密封直至线上；S4 PutSecret 在自己边界内转 Zeroizing。
    pub fn resolve(&self, reference: &SecretRef) -> Result<Sealed<String>, AxError>;   // 未命中→E_CREDENTIAL_MISSING；恒不跨操作缓存
    pub fn describe(&self, reference: &SecretRef) -> Described;                        // 恒不返回值
}
pub struct Captured { pub replaced: Vec<u8>, pub events: Vec<Payload> }   // secret_captured 载荷（入账归调用方）
```

- 生产适配器＝keyring crate（Windows Credential Manager／macOS Keychain／Linux secret service）；第二适配器＝会话内存 BTreeMap（探测全败的兜底＋测试面）。**恒不自写加密文件**。
- realm/name 派生：capture 时 realm=形状表 provider（无则 "detected"）、name=定长计数器 `cap-<n>`（确定性，无随机）；用户改名属 S4 命令面。
- OAuth 两流程（PKCE／设备码）＝代码：`pub fn oauth_begin(profile, …) -> OauthPending`＋`pub fn oauth_redeem(pending, …) -> Sealed<String>` 的纯构造（HTTP 往返由调用方经 endpoint 的 Client 执行或 S4 命令面驱动；本期交付构造与校验，不交付活体登录）。续期＝到期前 resolve 触发 refresh 构造。
- 环境变量是只读来源（键形 `SPRAWLING_SECRET_<REALM>_<NAME>`）：`describe.writable=false`；`set` 撞遮蔽即拒并指名遮蔽者；读取器可注入（edition 2024 的 set_var 不安全，测试恒不改进程环境）。
- S3.03 落地记录：A13 值正确性经真 Endpoint＋回环假服务在线断言（capture→vault→resolve→写头，服务侧见原值）——credential 自身零 `.expose(`；PKCE 以 RFC 7636 Appendix B 向量钉实（S256，sha2 为外部协议事实非第二哈希权威）；base64url／percent-encode 自写纯函数；probe() 对真平台服务的验证属装配期人工清单（测试不擅动开发者凭证库）。

### 8-5 gateway::oauth_profiles（S3.03；形状 6 数据面）

```rust
pub struct OauthProfile { pub provider: &'static str, pub auth_endpoint: &'static str,
                          pub token_endpoint: &'static str, pub scopes: &'static [&'static str],
                          pub client_id: &'static str, pub headers: &'static [(&'static str, &'static str)] }
pub const OAUTH_PROFILES: [OauthProfile; N] = [ /* 各 provider 一行；只有数据零分支 */ ];
```

- 内容自活跃维护的开源 harness 情报汇集（跟情报不跟代码）；上游变更检测是周任务不是 CI 门。S3 初版收录 Anthropic／OpenAI 两行（可空流程字段留空串——宁缺毋错，缺项在 oauth_begin 处 fail-closed）。
- **情报源两家（P4.00 用户裁定）**：codex 管 OpenAI 一侧，pi 管 Anthropic 与其余订阅 provider；名单与复核办法住 `docs/third-party.md`。
- **R1.14 增（credential＋oauth_profiles）**：`pub fn oauth_redeem(profile, pending, code, timeout_ms) -> Result<OauthTokens, AxError>` 真正把 POST 发出去，`OauthTokens { access, refresh, expires_in_s }` 两个密文恒裹在 `Zeroizing` 里且 `Debug` 手写成 `<redacted>`——`Zeroizing` 自己的 `Debug` 会打印明文，派生一个就等于把活令牌交给第一条格式化它的 panic 信息。**发送住 credential 而不住 endpoint**：本模块就是整条 OAuth 流程，把「造请求」与「发请求」分到两个模块，就是让一次兑付有两个权威。**拒词恒不引用对侧正文**：令牌端点的错误页里可能带着刚用过的 code。`OauthProfile` 增 `api_base`（该 provider 的 API 根，登录完成后据它自动 attach；空串＝fail-closed，与空端点同口径）。
- **R1.18 增（credential）**：`oauth_refresh(profile, refresh: &Sealed<String>, timeout_ms)` 与兑付**共用一次发送**（`send_token_request`），故「拒词不引用对侧正文」只写一次、也只可能对一次。入参是 `Sealed<String>` 而不是 `&str`：明文只在**线前最后一格**出现，这与 endpoint／native 是同一类兑付点，故本文件同期进 `xtask secret` 的 expose 白名单——**放宽白名单而不是在调用点绕开它**，因为绕开的写法会让装配层持明文，而那正是这张名单存在的理由。
- **`state != code_verifier`（P4.09，在 `credential::oauth_begin` 执行）**：两个值答的是不同的问题——verifier 证明「来兑的就是当初请求的那个客户端」，state 证明「这次回跳对应本进程发起的那次请求」。互用就是把一个证明做两遍、另一个一遍不做，且已有 provider 直接以 `400 invalid_grant` 拒。**在构造点拒而不交给接线的人记住**：这正是一份照上游抄来的实现会具有的形状。
- **迁出为独立 crate：本期未做，理由写在这里**。迁出的前提是一个**仓库外**的、持自有许可的上游存在；它今天不存在。在本仓库里建一个「另一个许可的目录」只会同时得到两件坏事：MPL 头门要么被改得认不出它、要么给它戟上一顶不属于它的帽子，而两者都不是迁出。本期因此只做两件真实可做的：表缺失时恒三段式拒（已在），以及 NOTICE 义务入 release 卡（P4.14）。

### 8-6 gateway::admission（S3.04；形状 1 判定函数）

```rust
pub struct AdmissionState { /* in_flight: u32、interval_ms: u64、consecutive_ok: u32、next_allowed_at: TimeMs —— 字段私有，构造子给初值 */ }
#[non_exhaustive] pub enum AdmissionVerdict { Admit, Hold { until: TimeMs } }
pub fn admit(state: &AdmissionState, now: TimeMs) -> AdmissionVerdict;      // 纯判定：不改 state
pub fn on_dispatch(state: &mut AdmissionState, now: TimeMs) -> Result<(), AxError>;
pub fn on_outcome(state: &mut AdmissionState, outcome: ProviderOutcome, now: TimeMs) -> Result<(), AxError>;
#[non_exhaustive] pub enum ProviderOutcome { Ok, RateLimited { retry_after_ms: Option<u64> }, Failed }
```

- 确定性 AIMD：RateLimited→interval 加倍（上限封顶；retry_after 在场则取其大者）；连续 `ADMISSION_OK_STREAK=8` 次 Ok→interval 减半（下限 `ADMISSION_MIN_INTERVAL_MS=250`）；并发上限 `ADMISSION_MAX_IN_FLIGHT=4`。三常量为 pub(crate) 数据面（provider 侧工程参数，非城策口径，不入 consts_policy；改须本 SPEC 同集）。
- S3.04 落地记录：`on_dispatch` 对越帽调用 fail-closed 后拦（E_BUDGET_EXHAUSTED 形）；`ADMISSION_MAX_INTERVAL_MS=60000` 封顶；快照回滚＝持前值（值语义，测试钉实）；结算溢出恒拒不回绕。
- 无时钟采样：`now` 恒入参；同一到达序列重演逐条同 verdict（验收 §2）。

### 8-7 gateway::market（S3.04；形状 6 数据面＋快照）

```rust
pub struct ModelEntry { pub id: String, pub context_tokens: u64,
                        pub input_price: UsdMicros /* per 1M tokens */, pub output_price: UsdMicros,
                        pub cache_read_price: UsdMicros, pub cache_write_price: UsdMicros }
pub struct MarketSnapshot { /* version: u32、entries: BTreeMap<String, ModelEntry> —— 私有 */ }
impl MarketSnapshot {
    pub fn builtin() -> MarketSnapshot;                                  // 内置钉版目录（数据面）
    pub fn from_entries(version: u32, entries: Vec<ModelEntry>) -> Result<MarketSnapshot, AxError>;
    pub fn lookup(&self, id: &str) -> Option<&ModelEntry>;  pub fn version(&self) -> u32;
}
```

- 钉版回滚＝持前一快照即回滚（值语义，无 I/O）；快照落盘属 projection／config 面，本模块只管形与查询。价目恒整数微美元（判定路径禁浮点）。

### 8-8 gateway::cost（S3.04；形状 1 判定函数）

```rust
pub struct CallCost { pub billed: UsdMicros, pub source: CostSource, pub usage: ModelUsage }
#[non_exhaustive] pub enum CostSource { Authoritative, PriceSheet }
pub fn settle(usage: &ModelUsage, authoritative: Option<UsdMicros>, entry: &ModelEntry) -> Result<CallCost, AxError>;
```

- 权威计费额在场恒胜（`CostSource::Authoritative`）；缺席则按价目推算：`input×input_price/1M + output×output_price/1M + cache 两项`，全程 checked 整数（溢出→E_INVALID_ARGS 报「结算溢出」）。model_returned 载荷含 `billed_usd_micros`＋usage 四整数，A20 对账消费之。

### 8-9 gateway::router（P1.11；形状 7 projection）

```rust
pub struct AttachedEndpoint { pub name, pub base_url, pub dialect: DialectKind,
                              pub auth: AuthSpec, pub models: Vec<String> }
impl AttachedEndpoint {
    pub fn is_local(&self) -> bool;          // 与本地适配器同一判据（native::is_loopback）
    pub fn has_credential(&self) -> bool;    // 关于凭证，金库外只能回答这一问
    pub fn chat_url(&self) -> String;        // base_url ＋ 方言自己的路径
    pub fn models_url(&self) -> String;
}
pub struct EndpointBook { /* 私有：endpoints、chosen */ }
impl EndpointBook {
    pub fn apply(&mut self, record: &EventRecord) -> Result<(), AxError>;
    pub fn apply_payload(&mut self, kind: EventKind, data: &Payload) -> Result<(), AxError>;
    pub fn select(&self, tag: ModelTag, policy: &BuildingPolicy) -> Result<Chosen<'_>, AxError>;
    pub fn endpoints(&self) -> impl Iterator<Item = &AttachedEndpoint>;
    pub fn choices(&self) -> impl Iterator<Item = (ModelTag, &str, &ModelEntry)>;
}
pub fn attached_payload(&AttachedEndpoint) -> Result<Payload, AxError>;      // endpoint_attached 唯一成形处
pub fn selected_payload(ModelTag, &str, &ModelEntry) -> Result<Payload, AxError>;  // model_selected 同上
```

- **为何不是 duty pool**：多 Agent 功能未成形之前，职责池没有消费者，而没人读的权威只会漂。降为 `ModelTag` 两值枚举（`Main`／`Digest`）：**标签因为有人按它取模型而存在**，新增一个标签的前提是先有调用方。
- **两个入口一个读者**：`apply`（重建路径，手里是 record）与 `apply_payload`（写入路径，手里是刚要写的 payload）共用同一套载荷读取，于是「写者以为的」与「重建得到的」不可能分岔。
- **confidential 在选型点再守一次**：非本机 endpoint 对 confidential 楼恒拒（`E_GATE_DENIED`）。`gateway::endpoint` 的兜底拒同期改为**按本地性判定**（而非一律拒）：规则是「字节不出本机」，不是「不准用这个类型」；否则一个回环的 Anthropic 服务器会被误拒。
- **路径归方言**：人输入 base URL（provider 文档就是那么印的），`messages`／`chat/completions`／`models` 由方言拼。这与 `EndpointConfig.base_url`「完整端点 URL、不拼路径」并不矛盾：适配器保持字面，拼路径的是上层登记面。

### 8-10 gateway::endpoint 探测面（P1.11）

```rust
impl Endpoint { pub fn list_models(&self, url: &str) -> Result<Vec<String>, AxError>; }
```

两家都在 `GET .../models` 返回 `{"data":[{"id":..}]}`，**都不返回价目与 token 上限**——所以探测只取 id，两个 token 数字由人在登记时确认。探测与正式调用共用同一个兑付路径（`authorize`）：两套认证拼法就是两个权威，而漂开的总是没人看的那个。

## 8.5 两个设计（crate 级）

**A（选中）：canonical 会话类型住 kernel::model 缝上，dialect 只做翻译**——ScriptModel（citysim）与真适配器消费同一请求形，重放重建的入窗字节有唯一权威；代价是 kernel 公开面变大（约十个纯数据类型）。
**B（落选）：canonical 类型住 gateway，Model::call 只收哈希、真会话经旁道传递**——kernel 面最小，但旁道即第二权威：sim 与真适配器走不同请求形，dialect 往返与 A15 重建无从对同一对象断言；缝的意义（「同一批产品 crate，换适配器整城可跑」）被掏空。落选。
**endpoint 客户端选型**：reqwest::blocking（选中）vs 自写 hyper 直连 vs ureq。自写 hyper＝维护整个连接池与 TLS 面，收益为零（我们自写的是**线格式**不是传输层）；ureq 更小但 rustls 集成与代理面弱于 reqwest；blocking 而非 async＝与同步 turn 面同构，避免为一个 HTTP 调用引入全库 async 传染。

## 9 工作流程

回合层组 `ChatRequest`（prefix 四段＋窗口历史）→admission.admit→endpoint.call（dialect 翻译＋兑付＋HTTP）→cost.settle→model_returned 载荷（usage＋billed）→attribution（memory 侧）摊回。credential 独立线：启动 probe→capture（Tainted::new 构造点驱动）→resolve（组请求末格）。

## 10 实现逻辑

dialect 先行（纯函数零依赖，golden 钉形）→endpoint 骨架（假 provider 服务回环测试）→credential（Vault 两适配器＋探测）→admission/market/cost（纯判定）。native 最后（复用 endpoint）。每步红先行：golden 未落前不写翻译分支。

## 11 边界枚举

空 messages（合法：首轮）；空 tools（不写 tools 键）；SSE 半流（S3 非流式先行，流式只加）；429 携 retry-after；usage 缺席（取 0，CostSource=PriceSheet）；权威计费额为 0（合法，免费档）；base_url 尾斜线；overrides 指向不存在的路径（创建）；OAuth profile 字段空串（oauth_begin fail-closed）；Vault 探测三候选全败（会话内存＋provider_degraded）；遮蔽写入；空串凭证（视同未配置）。

## 12 错误处理（逐码答「能否定义掉」）

- `E_PROVIDER`：不可定义掉——网络与对端是本 crate 的本质失败面；subject 写状态码与端点名，恒不含请求体。
- `E_WIRE_MISMATCH`：不可定义掉——对端响应形状漂移是外部事实；subject 写键路径。
- `E_ENDPOINT_DIALECT_UNSUPPORTED`：不可定义掉——用户可配任意 external provider，方言探查失败必须可报。
- `E_CREDENTIAL_MISSING`／`E_CONFIG_INVALID`：kernel 已有码，语义照 Custody 一节；不新增码。
- `E_SECRET_EGRESS`：本 crate 不产（出口扫描住 gate::egress 与 checkpoint 面）；endpoint 组请求不做二次扫描（Custody 在入口已换引用，纵深由门守）。

## 13 依赖选型

- `reqwest = { version = "0.13", default-features = false, features = ["blocking", "rustls-tls", "json"] }`——钉版表钉 rustls；blocking 理由见 §8.5。2026-08 复核：0.13.4 现行。
- `keyring = "3"`——平台凭证服务绑定（B.7「平台凭证服务绑定」行）；MIT/Apache。
- `secrecy`／`zeroize`：workspace 既钉。serde_json：wire 面。
- **tokio 不引**（偏离 B.7 引入期列，理由 §3；S4 channels 首个真异步消费者时引入）。

## 14 硬编码声明

admission 三常量（§8-6）；oauth_profiles 表（§8-5，数据面即定义处）；market 内置目录（`builtin()`，S3 收录城内实际使用的模型行，价目随 Stage 复核）。三者全 pub(crate) 数据面，改动须本 SPEC 同集变更。

## 15 影响面

kernel::model 增 canonical 会话类型（kernel-SPEC §8-24 同集改；specalign 若辖新枚举则表同集落）；runtime 回合层 S3.08 起消费 ChatRequest；memory::attribution 消费 model_returned 新载荷字段；citysim ScriptModel 改收 ChatRequest（同一缝）。

## 16 测试与约束

golden：两 Dialect 各一请求一响应（insta）；proptest：响应往返、usage 保值、断点位保序；endpoint 对回环假服务的状态码矩阵（200/429/500/截断体）；credential：内存 Vault 全流程＋探测注入；admission 重演等值；cost 权威胜出＋溢出拒绝。约束：clippy 零告警；无 `unwrap`；`.expose(` 只在 endpoint.rs／native.rs。

## 17 模型体验

零字节：gateway 全体不产 prefix 字节。间接贡献：dialect 保断点位使 city-wide 缓存在真 provider 上成立（省的是每回合重付的 prefix 费）；cost/attribution 使「钱花在哪」可答而不占模型上下文（成本归因是给人看的）。

## 18 文档同步

ARCHITECTURE §6 gateway 表逐卡状态翻转；§6 接线台账登记（endpoint/native 生产消费者＝S3 回合层与 assembly；credential 消费者＝endpoint＋S4 PutSecret）；kernel-SPEC §8-24 同集改（S3.01 落 canonical 类型时）；api-baseline 含 gateway 起算（SPEC 既存，apisync 自动入集）。
