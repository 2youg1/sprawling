# channels-SPEC.md

> crate：`channels`（lib，依赖 kernel）。本 SPEC 先于代码存在；实现不多不少地遵守本文。
> 骨架：十七节；按模块分章、每章自足。
> Stage 4 五模块：wire／server／control／auth／aggregate。
> 本 crate 覆盖的语义：wire 面（Command／Query／Event、编码与握手、绑定面）、干预五动词、多台机器一个界面、Autonomy 应答者、三队列。

## 1 需求拆解

| 卡 | 模块 | 一句话 |
|---|---|---|
| S4.02 | `wire` | Command 17／Query 9／Event 三分的类型与 JSON 编码；版本＋schema 哈希握手帧 |
| S4.02 | `server` | WebSocket 服务；静态资源与上传端点；绑定面判定（默认只绑回环） |
| S4.03 | `auth` | 本机零摩擦；非回环要求配对令牌且常数时间比较；未配置令牌即拒绝启动 |
| S4.03 | `control` | 人的五动词入口；自持鉴权与幂等（做不到则并入 `server`——ARCHITECTURE §6 已写明这条退路） |
| S4.04 | `aggregate` | 多 City 只读聚合：只转发 Query 与 Event，恒不转发 Command |

**本 crate 是进程外边界的唯一守卫**。它不实现任何业务判定：Command 的执行、Query 的求值、Event 的产生全在上游（runtime／memory／city），本 crate 只负责「让非法的帧在类型层或握手层就不存在」。

## 2 验收标准

- **wire**：Command 恰 18 个 variant（P4.08 增 `Wake`）、Query 恰 14 个（计数断言，对本 SPEC §8-1 两表逐名核对）；每个改状态 Command 携 `IdemKey`（类型强制，无可省字段）；`PutSecret` 的 `value: Sealed<String>` 不实现 `Serialize`——**「远程录凭证」这条帧编译不出来**，以 trybuild 反例钉死。
- **握手**：版本＋schema 哈希不配即断连并回 `E_WIRE_MISMATCH`（装载期码，无 carrier）；schema 哈希由 wire 类型集派生，改一个 variant 即变。golden 钉住当前哈希，改哈希必须与本 SPEC 同集变更。
  **当前 golden**（V3.13 起）：`4ac1b7b375c9a944a32920129e703673fe3d0092c3cea22c44da411f10855aac`；**WIRE_V ＝ 12**（新增 `ServerFrame::Delta`）。前值 `78fdb74d…`（ux-14，WIRE_V 11：新增 `Query::Changes`）、 `1de1a1ae…`（ux-13，WIRE_V 10：新增 `Query::RunHistory`）、`c7b41d50…`（P3.04，WIRE_V 9：新增 `Query::History`）、`0a600659…`（P3.02，WIRE_V 8：新增 `ConfigureBuilding`）、`4bb71c0b…`（P3.01，WIRE_V 7：新增 `ProbeEndpoint`，`AttachEndpoint` 长出 `admit`）、`c059c6e2…`（F2.16–P2.01，WIRE_V 6）、`d825e83a…`（F2.11–F2.15，WIRE_V 5）、 `aa57cb7e…`（F1.01–F2.10，WIRE_V 4）、 `941ede9f…`（R1.16–R1.18，WIRE_V 3）、`defe9a75…`（R1.14–R1.15，WIRE_V 2）、 `85705c03…`（R1.11–R1.13，WIRE_V 1）、`238f11b2…`（P1.11–R1.10）、`692b5f96…`（S4.02–P1.10）。
  P1.11 增三帧：`AttachEndpoint`／`SelectModel` 两个 Command（十九），`EndpointView` 一个 Query（十）。`PutSecret` 仍无线格式——它经 `/enroll` 路由在进程内成形，见 §8-2 录入口。

**ux-13 增：`Query::RunHistory { run, before, limit }` → `Answer::History`，WIRE_V 9→10。**

补的是一个**页面成立的前提**而不是一项便利：`Query::History` 不带 run 过滤，客户端在连上时问一次城全局的最后 500 条，然后在本地按 run 过滤。于是四个会话分这 500 条，而**昨天的会话根本不在里面——打开它是一张空白页**。`Query::RunView` 只答 5 个字段，它回答「这个 run 在不在、走到哪」，不回答「这个会话是什么」。

**答面复用 `HistoryAnswer` 而不新增一个。** 它已经带 `earlier` 游标，形状正是「往回翻」；再造一个只会让「一页历史长什么样」有两个答案。

**~~扫描窗口有界，且与返回条数是两个界。~~**（ux-13 的原文：一个上月结束的会话，若按「一直往回扫到凑够 limit 条」实现，就是客户端可以驱动的无界服务端工作，故新增 `HISTORY_SCAN` 作为第二个界，于是一个答可以是空的而 `earlier` 是 `Some`。）

**V3.03 删去 `HISTORY_SCAN`，因为它再也不保护任何东西。** 它存在的理由是「扫描量与账本长度同阶」；`memory::index` 持了 run 索引之后，一次 `RunHistory` 只取属于这个 run 的 seq，**扫描量与 `limit` 同阶**，而 `limit` 已由 `HISTORY_MAX` 封顶。留下一个不再防任何事的上限，会让下一个读代码的人以为它还在防什么。

**于是 `earlier` 的含义变精确而不变形**：仍然是「从这条之前接着问」，`None` 仍然是「到头了」；只是「答是空的而 `earlier` 是 `Some`」这一态**再也不会出现**——服务端不再需要把「我这一段没扫到」告诉客户端。线格式一字未动（`HistoryAnswer` 两个字段原样，`WIRE_V` 不变），客户端的翻页循环不需修改；变的只是它不再收到空页。

**ux-15 增：转出 `kernel::{Span, Token, markdown}`，线格式未动。** 客户端只看得见
本 crate，而文档的词法器在 kernel（无 I/O、无时钟、输出穷举枚）。转出一个纯函数而不是
新增一个 Query：分词在浏览器里跑就行，为一个几 KB 的词法器先把线格式撑大，
是替尚不存在的第二实现造抽象。真需要语法引擎时它去服务端，线格式那时再长。

**~~不建 run→seq 的索引。~~**（ux-13 的理由：那会是「这个 run 的历史在哪」的第二个权威，而账本本身已经是第一个；有界扫描付的是几毫秒的解析，索引付的是一份要随账本同步的派生状态。）

**V3.03 推翻这一条，因为它的两条理由各自的参数都动了。** 其一，「几毫秒的解析」是估的：实测一次 `RunHistory` 在 5 万条账本上是 **2823 ms**，V3.01＋V3.02 把它压到 **22.4 ms**，仍然比估值高一个量级，而这是「打开昨天的会话」这个动作的全部延迟。其二，「一份要随账本同步的派生状态」在 V3.02 之后已经存在：`memory::LedgerIndex` 常驻于 `Views` 并每次查询 `refresh`，run 表只是它多一个字段，搭同一趟刷新、同一份 cache、同一条「存疑即重建」的反射，不新增同步义务。至于「第二个权威」：索引回答的是「在哪」，从不回答「是什么」，它可弃且存疑即重建；账本仍是唯一权威。接面与内存代价见 memory-SPEC §8-4。
- **server**：默认绑定回环；绑非回环且 `auth` 未配置令牌时**拒绝启动**并回 `E_CONFIG_INVALID`（不是启动后再拒连——这是绑定面判定，不是请求面判定）。
- **auth**：令牌比较恒为常数时间（不早退）；比较函数以「逐字节差异位置不影响耗时」的性质测试看守。S4.02 已落其地基（`server::constant_time_eq`＋`decide_handshake`）；S4.03 的 `auth` 模块接令牌的生成、展示与持久化。
- **aggregate**：**类型化保证**——聚合上游连接的发送面在类型上只接受 `Query`，没有一个能塞进 `Command` 的方法（不是运行时 `if`，是类型上不存在该入口）；以 trybuild 反例钉死。
- **上传端点**：`Attach` 的字节走 HTTP，不走 WebSocket 帧；命令语义不变（明写这是传输细节）。

## 3 假设与歧义

- **本期首次引 tokio**。S3 已记「不引 tokio，endpoint 用 `reqwest::blocking`；tokio 行推迟到 S4 channels——首个真异步消费者」（gateway-SPEC §3／§13）。本卡兑现该推迟，**须携 `Verdict:` 尾注**（触碰根 `Cargo.toml`，guard 辖区）。
- **没有第二条传输路径**：即使浏览器与服务在同一台机器上也是网络连接，故不存在「同进程内存通道」这条优惠。唯一例外是 `PutSecret`——它不是靠运行时判断走内存通道，而是**类型上不可序列化**，因此远程连接根本编不出这条帧。
- **编码是 JSON，且编码本身不进冻结面**。选 JSON 的理由是不对称：浏览器原生支持，且开发者能在网络面板直接读帧。
- **`control` 是否自持鉴权与幂等——已作答（S4.03）：独立成模块**。ARCHITECTURE §6 预留了「做不到则并入 server」的退路，本卡不需要它：`control` 持有一条 `server` 不知道也不该知道的策略——**哪些 Command 是干预，以及一次干预必须留下什么**（「任何中断都以 Handoff 收尾，下一位拿得到完整现场」）。那是判定，不是转调。
- **`auth` 同时收回了 S4.02 放错位置的一块逻辑**：常数时间比较当时写在 `server` 里，那是因为 `auth` 尚未建。本卡把令牌的**整个生命周期**（铸造、展示形、摘要、比对）收进 `auth`，`server::decide_handshake` 改为调用它。这不是重构的赔罪，是模块建成后把属于它的东西放回去。
- **Signal 不在 Command 面**：`Attach{notify}` 产 Signal，但 Signal 的投递与消费住 `collab::inbox`（P2）。channels 只是产地。

## 4 现状分析

空壳 lib（`src/lib.rs` 仅 crate 文档）。无既有公开面，api-baseline 自本期起算。下游唯一消费者是 `web`（ARCHITECTURE §2 depmap：`web: channels`），装配消费者是 `crates/sprawling`（bin `serve`）。

## 5 权威信源

wire 面全节（Command 表、Query 表、编码与握手、绑定面三段）；聚合层硬约束「聚合层只转发 Query 与 Event，恒不转发 Command」；五动词语义表；`Sealed<T>` 的不可序列化性质；`kernel::error` 的装载期五码白名单（kernel-SPEC §8-1，封闭且不得增长）。外部：axum 0.8.9（2026-04-14，内含 tokio-tungstenite 0.29）与 tokio。

## 6 命名统一

Command／Query／Event（三分的原名，不译）；Dispatch／Login／Fork／Attach／CreateBuilding／PutSecret／Steer／Cancel／Takeover／Rollback／Halt／Release／BatchByBuilding／Approve／CreatePolicy／SetAutonomy／Auth／Wake（命令原名，逐字取本 SPEC §8-1 表）；RunView／CityView／ApprovalQueue／InboxView／Metrics／CostView／ArchiveSearch／RegistryView／DiscardView（9 查询原名）；control surface（不译）；配对令牌＝pairing token；握手＝handshake。

## 7 模块边界

```
                    ┌── wire（类型与编码；无 I/O，纯数据与纯函数）
server（tokio＋axum）┤
  ├ WS 端点         ├── auth（令牌判定；常数时间比较）
  ├ 静态资源         └── control（五动词入口；鉴权与幂等）
  └ 上传端点（HTTP）
aggregate ──▶ 上游 City 的 WS 连接（发送面类型上只收 Query）
```

**不做什么**：不执行 Command（只解码并交给装配层注入的处理器）；不求值 Query（同上）；不产 Event（Event 载荷即 `EventRecord`，产地在各效果模块）；不做业务鉴权之外的策略；不声明 `pub` trait（不在 ARCHITECTURE §3 缝清单内——**处理器以函数指针或具体类型注入，不是端口**）；不内置任何穿透或中继（明拒：「内置一种就是替用户做了一个安全决定」）。

## 8 接口先行（按模块分章）

三条不可动摇的形状约定，它们决定接口而非被接口决定：

1. **`Command` 是穷尽枚举，每个改状态臂携 `IdemKey`**——「双击两下不开两个 Run」由类型保证，不由服务端去重表保证（去重表是第二道，`kernel::gate::dedup` 已有）。
2. **`PutSecret::value: Sealed<String>`**——`Sealed<T>` 无 `Serialize`（kernel-SPEC §8-25），故含它的枚举也无法整体派生 `Serialize`。这迫使 `Command` 的序列化实现**手写并对该臂显式拒绝**，而不是让宏悄悄地把它序列化出去。手写点即唯一权威，`E_WIRE_MISMATCH` 在此产出。
3. **`aggregate` 的上游发送面签名只接受 `Query`**——不是 `fn send(&self, frame: Frame)` 再运行时判断，是 `fn query(&self, q: Query)` 且没有第二个发送方法。

### 8-0 跨层名字的携带法（先于一切接口的决定）

`Dispatch` 携 `mode`、`Login` 携 `provider`、`CreateBuilding` 携 `template`——**这三个集合的权威分别住 `runtime::Mode`、`gateway`、`city`，而 channels 只依赖 kernel**（ARCHITECTURE §2 depmap）。

取法：wire 携**无封闭列表的 newtype**（`ModeTag`、`ProviderName`、`TemplateName`），只断言「非空且无控制字符」，**不断言合法值集**。合法值集恒由上游单一权威回答，映射点在装配层（`bin::assembly`），未知值即报错不猜。

理由：若 channels 自建一份 `enum Mode`，就产生了**同一规则的第二个权威**（AGENTS.md 明拒），且两份枚举会静默地漂开。channels **确实不知道** mode 集合是什么，假装知道才是谎言。**被否**：（a）channels 内镜像三个枚举——两个权威；（b）把 `Mode` 上移入 kernel——它不在任何缝上，上移只为了让 wire 好看，且删 `runtime::mode` 行需 verdict。

### 8-1 channels::wire（S4.02；形状 2 值类型 ＋ 形状 1 编解码）

```rust
pub struct ModeTag(String);      // 三个携带 newtype：parse 拒空串与控制字符，不拒未知值
pub struct ProviderName(String);
pub struct TemplateName(String);
pub enum HaltScope { City, Building(Address), Workshop(Address) }  // 协议自有，无外部权威

pub enum Command { /* 逐名取本 SPEC §8-1 表 */ }
pub enum Query   { /* 息 9 */ }

impl Command {
    pub fn name(&self) -> &'static str;   // 穷尽 match：加一个 variant 即编译不过
    pub fn idem(&self) -> Option<&IdemKey>; // 改状态臂恒 Some；唯一例外 Auth
}
impl Query { pub fn name(&self) -> &'static str; }

pub const WIRE_V: u32;
pub const COMMAND_NAMES: [&str; 17];   // 形状 6 数据面：名字权威
pub const QUERY_NAMES:   [&str; 9];
pub fn schema_hash() -> B3Hash;        // blake3("sprawling/wire/" || WIRE_V 小端 || 'C'+名… || 'Q'+名…)

pub enum ClientFrame { Hello(Hello), Command(Box<Command>), Query(Query) }
pub enum ServerFrame { Welcome(Welcome), Event(Box<EventRecord>), Reply(Box<Reply>), Refusal(Box<AxError>) }
pub struct Hello   { pub wire_v: u32, pub schema: B3Hash, pub token: Option<Sealed<String>> }
pub struct Welcome { pub wire_v: u32, pub schema: B3Hash, pub resume_from: Option<Seq> }
```

**三个形状决定及其理由**：

1. **`Command`／`Query` 不标 `#[non_exhaustive]`**。它们的版本机制是 schema 哈希（加一个 variant 即改哈希，旧客户端在握手处被拒），不是通配臂。不标它使装配层必须**穷尽处理 18 条命令**——新增一条而无人处理在编译期即红。这是想要的约束，不是疏漏。
- **`Query::History` 有界且向后翻页（P3.04）**：服务端只广播「接下来发生什么」，于是今天打开的页面把一座运行了一个月的城看成空城。答复恒**旧在前**——那是账本写它们的顺序，也是折叠期待的顺序；要新在前的读者自己倒一下手上的表，而服务端倒序会把折叠变成调用方的问题。上限 `HISTORY_MAX = 500` 由服务端钳，客户端要不到更多：一个调用方钳不动的上限，少一条让服务端做无界工作的路。`Answer` 因此失去 `Eq`（`EventRecord` 的载荷是任意 JSON，JSON 没有全序相等），crate 外无人比较过两个 `Answer`。
- **`/enroll` 答的是凭据的下场，不是请求的下场（P3.03）**：先前命令一进桌就回 201，于是 vault 拒收谁也不知道，人盯着一条成功消息而密钥根本没存。现在路由**先订阅事件流再投递命令**（顺序是承载的：先投递会让完成得快的 worker 把那一行写进没人在读的流里），然后等三选一——`secret_captured` 且 `ref` 相符即 **201**，`Reply` 回来的拒绝即 **422**，两者都没有即 **202 并在正文里说明为什么是 202**。`SecretSink` 因此长出 `Reply` 参数：worker 在另一条线程上几分钟后拒绝，没有地址的拒绝到不了任何人。
- **回复通道关闭不等于成功（P3.03）**：worker 成功时不调用 `reply.refuse`，`Reply` 随之析构，于是 `recv()` 立即返回 `None`。若把它读成一个答案，它会与 `secret_captured` 赛跑并经常赢——所以关闭只熄灭那条分支，201 仍然只由事件给出。
- **`ConfigureBuilding` 写的是楼自己那一级（P3.02）**：`[sandbox]` 与 `[mcp]` 沿城→楼→房间解析，先前无任何写面，于是一个人读得到自己被什么治理却改不动它。答复里回的是**楼自己那一级的值**而不是解析后的值——用解析值填表，第一次按保存就会把城一级的设置抄进楼里。两个字段各自可缺省，缺省即不动那一节；`mcp` 为空表是「这栋楼一个服务器都不够到」，与「没说」不是一回事。
- **`ProbeEndpoint` 与 `AttachEndpoint` 是两条命令而不是一个开关（P3.01）**：先前模型清单只作为 attach 的副产物到达，于是「看看这把 key 买到了什么」必须先注册。两者的 `IdemKey` 由不同素材派生（`probe:` 前缀），因为问与登记是两件事，一件不得把另一件去重掉。`AttachEndpoint.admit` 空表即全部准入——没看过清单的人本就是这个意思；表里有而 endpoint 不供应的名字被略去而不是被承诺，与阅览室对不在书架上的 skill 的答复同形。

2. **`name()` 的穷尽 match 是计数断言的真机制**。光有 `COMMAND_NAMES.len() == 17` 拦不住「加 variant 但不改表」；`name()` 穷尽后，新 variant 必须在 `name()` 里现身，测试再断言它必在名表内。三道连环：编译 → 名表 → schema 哈希 golden → SPEC 同集变更。
3. **`Command` 泛型于 secret 携带者，远端实例把它钉成不可居住类型**（S4.02 施工中修正的写法，强于本 SPEC 初版的「手写 Serialize 在该臂报错」）：

```rust
pub enum NoSecret {}                                   // 无值可造
pub enum Command<Secret = Sealed<String>> { … PutSecret { value: Secret } … }
pub type WireCommand = Command<NoSecret>;              // 套接字所能携的全部
impl From<WireCommand> for Command                     // 总函数；PutSecret 臂写作 `match value {}`
```

它同时关死两个方向，**且两边都是编译期**：出——`Sealed<String>` 无 `Serialize`，故 derive 生成的 `impl<S: Serialize> Serialize for Command<S>` 对 `Command<Sealed<String>>` 不成立，`serde_json::to_string` 对它是编译错误；入——`WireCommand::PutSecret` 的 `value` 字段无值可填，任何类型都不匹配。运行期只剩一道兑底：字节写着 `put_secret` 时 `Deserialize` 拒收，**拒绝文案恒不回显它正在保护的字节**。两个编译期反例住 `tests/ui/put_secret_onto_the_wire.rs`（stderr 快照分别钉在 `Sealed: !Serialize` 与类型不匹配两个正因）。

**被否**：另写一个 16 variant 的 `WireCommand` 枚举——它把 16 条命令的声明拄成两份，是同一规则的第二个权威。

4. **`Auth`／`Hello` 的令牌是明文 `String`，而 `PutSecret` 的值是 `Sealed`**。不对称是故意的：配对令牌**必须跨线**才能完成配对，在传输中密封它只是自欺；它在**落地一刻**被封（`decide_handshake` 只接受 `&Sealed<String>` 作为已配置值）。凭证则相反：它本就不应跨线。

5. **`Rollback{checkpoint}` 携 `kernel::GitOid`，为此给 `GitOid` 补 serde**（与紧邻的 `B3Hash` 同形：40 位小写 hex，长度不对即拒）。**被否**：在 wire 里自建 `CheckpointRef(String)` 并自校 40 hex——那是 git oid 形状的第二个权威。该变更属 kernel 公开面，已与 kernel-SPEC §8-2 同集提交（apisync 门）。

### 8-2 channels::server（S4.02；形状 1 判定函数 ＋ 形状 4 薄壳）

**Humble Object 在此的切法**（ARCHITECTURE §7 末段，理由只写一次）：难测的一端（tokio＋axum 监听）剥到最薄，厄的一端（绑定面判定、握手判定）是纯函数，无需跑服务即可穷尽测。

```rust
pub enum BindFace { Loopback, Exposed }              // 穷尽，不是 bool
pub enum BindVerdict { Serve(BindFace), Refuse(AxError) }
pub fn decide_bind(addr: &SocketAddr, token_configured: bool) -> BindVerdict;

pub enum EnrollVerdict { Accept, Refuse(AxError) }    // P1.11
pub fn decide_enroll(peer: &SocketAddr) -> EnrollVerdict;   // 只认本机调用方
pub type SecretSink = Arc<dyn Fn(Command<Sealed<String>>) -> Result<(), AxError> + Send + Sync>;
pub struct EnrollBody { pub realm: String, pub name: String, pub value: String }

pub enum HandshakeVerdict { Accept, Reject(AxError) }
pub fn decide_handshake(hello: &Hello, expected: &Welcome, configured: Option<&B3Hash>) -> HandshakeVerdict;

pub struct ServeConfig {
    pub addr: SocketAddr,
    pub token_digest: Option<B3Hash>,   // 摘要，不是令牌
    pub client: Arc<ClientAssets>,      // 客户端资产源由装配层递入
    pub upload_sink: Arc<dyn Fn(Bytes) -> Result<UploadId, AxError> + Send + Sync>,
}

// 补遗（P4 之后的整修卡）：客户端资产面。
// 判定纯函数化（Humble Object 同款切法）：哪个路径答哪些字节、要不要
// Content-Encoding、为什么 miss，全部离线可测；handler 是三行壳。
pub struct EmbeddedFile { pub path: &'static str, pub gz: &'static [u8] }
pub enum ClientAssets {
    Embedded(&'static [EmbeddedFile]),  // 发布形：二进制内的 gzip 文件表
    Disk(PathBuf),                      // 开发形：--web-dir 逐请求读盘
}
pub enum AssetReply {
    Found { bytes: Vec<u8>, content_type: &'static str, gzipped: bool },
    Miss(AxError),
}
impl ClientAssets { pub fn lookup(&self, request_path: &str) -> AssetReply; }
```

**为什么 `index_html: Arc<[u8]>` 改成 `client: Arc<ClientAssets>`（整修卡，落地即改这一处）**：原形状只能携带一个文件，而真实客户端是 `index.html` ＋ `web.js` ＋ `web_bg.wasm` ＋ wasm-bindgen snippets——单文件形状使「单二进制交付」在 P0–P4 里从未真的成立（页面壳引用 `./web.js`，服务端却没有那条路由，浏览器拿到的是空页）。资产表是封闭清单：路径穿越（`..`、空段、盘符、点头文件）在判定层拒，miss 报文件名并给出重建口令。`Disk` 臂逐请求读盘，专供开发回路（改前端刷新即见），发布路径恒不构造它。

**`upload_sink` 收 `Vec<u8>`，不收传输层的缓冲类型**（S5.01 接 bin `serve` 时发现）。初版写的是 `axum::body::Bytes`，于是装配层为了递一个 sink 就必须直接命名 axum。**一个泄露自己传输层的公开签名，会把「换掉 HTTP 库」变成对每一个从未选过它的调用方的破坏性变更**。

**令牌只以摘要形式进入本 crate**（S4.02 施工中由 `xtask secret` 门逆推出的修正）。初版写的是 `Option<&Sealed<String>>` 加一次 `.expose()`，门当场咬住——expose 只得出现在兑付点。**修因不修门**：拿令牌的一方自己摘一次，边界只比摘要。代价为零（常数时间比较本来就要先摘），收益是 `channels` 在类型上根本拿不到配对令牌的明文。常数时间比较因此退化为定长 32 字节的无早退异或，**既无内容侧道也无长度侧道**。

`decide_bind` 的四格真值表是全部行为：回环×无令牌＝`Serve(Loopback)`；回环×有令牌＝`Serve(Loopback)`；非回环×有令牌＝`Serve(Exposed)`；**非回环×无令牌＝`Refuse(E_CONFIG_INVALID)`**。拒绝发生在**启动时**，不是启动后拒连——它是配置判定。

薄壳的职责恒为三件：静态资源（`include_bytes!` 的前端产物）｜WS 升级｜上传端点（`Attach` 的字节，不走 WS 帧）。它不持业务状态，不做策略判断。

**P1.02 增：WS 路由与两条沿途缝**。升级后的会话只做三件事：先收 `Hello` 并交 `decide_handshake` 判（拒即关，不降级）；收到 `ClientFrame::Command` 交给 sink；把订阅到的 `EventRecord` 以 `ServerFrame::Event` 推给客户端。

```rust
pub struct ServeConfig {
    /* …前四项不变… */
    /// 命令受理面：**只受理，不执行**。同步、不阻塞；真正的回合循环在装配层自己的任务里跑。
    pub commands: Arc<dyn Fn(WireCommand) -> Result<(), AxError> + Send + Sync>,
    /// 事件广播源。本 crate 只 `subscribe`，恒不发送——写入方是 Ledger。
    pub events: broadcast::Sender<EventRecord>,
}
```

- **为什么 sink 只受理不执行**：一个 Dispatch 会跑几分钟到几小时。把它做成 `async` 并在 socket 任务里 await，等于把一条连接的寿命绑在一次派活上；刷新页面就会杀掉工作。**受理后立即返回，进展从 Ledger 的事件流回流**——这同时使「关掉界面再打开」与「从未关过」在服务端看来无差别。
- **为什么广播的是 `EventRecord` 而不是自定义推送体**：客户端要重建的正是那一行历史。另造一个推送类型等于为同一件事立第二个形状权威，而两者一旦漂开，界面会显示一个历史里没有的事实。
**P2.01 增：回信地址（`Reply`／`Delivered`）**。受理与执行分开之后，工人的拒绝没有任何通道回到发问的那个 peer——回程只有 `EventRecord` 广播。真机派活验出的后果是：**一个人在设置页点 attach，base_url 少了 `/v1`，页面一个字都不说**，那条拒绝只躺在服务端自己的日志里。

```rust
/// 一条拒绝的去向，三态穷尽。
pub enum Delivered { ToThePeer, NobodyAsked, PeerGone }

/// 回信地址。`Fn` 而非 tokio 通道，故本类型不把传输层写进签名。
pub struct Reply(/* private */);
impl Reply {
    pub fn to(sink: impl Fn(AxError) -> Delivered + Send + Sync + 'static) -> Reply;
    pub fn nowhere() -> Reply;                    // 排程自己发起的活，没有发问者
    #[must_use] pub fn refuse(&self, error: AxError) -> Delivered;
}

pub commands: Arc<dyn Fn(WireCommand, Reply) -> Result<(), AxError> + Send + Sync>,
```

- **拒绝属于发问者，不广播**。把它做成一条事件会告诉所有在看的人「别人打错了一个字」，而事件流是这座城的历史，不是某个人的错字簿。故每条会话自持一个无界队列，`Deliver` 时把写入该队列的闭包随命令交给工人；会话的 `select!` 因此从两臂变三臂。
- **`Delivered` 是三态而不是 `Result`**，因为「没有人问过」与「问的人走了」是两件不同的事：前者是排程的正常形态，后者值一行诊断。这也是本卡**不得重新引入 `let _ =`** 的落法——`SendError` 被穷尽消解成一个领域枚举，而不是被丢掉。
- **无界队列而非 `broadcast`**：一条拒绝丢不得，而它的量级是「人点错的次数」，不是事件流量。
- **未做且已知**：`/enroll` 路由同病。它同步答 201，而 `PutSecret` 是投递到同一张桌子的，工人的拒绝到不了 HTTP 响应。此处**不顺手改**，因为桌子在一次 dispatch 期间不被读取，把 HTTP 请求做成同步等待会让它挂上几分钟；正确的形状是有界等待加 202，随 P3 的 `sprawling enrol` 一并落。

**P1.03 增：Query 的答面**。`ServeConfig.queries: Arc<dyn Fn(Query) -> Result<Answer, AxError> + Send + Sync>`，同步；`ServerFrame` 增 `Answer(Box<Answer>)` 变体。答面类型住 `wire`：`Answer`（City／Run／Approvals／Cost／Unavailable）、`RunSummary`、`CityAnswer`、`ApprovalsAnswer`、`CostAnswer`。

**R1.11 增：`Query::BuildingView { addr }` → `Answer::Building(Box<BuildingAnswer>)`**（`BuildingDoc`／`ArchiveLine` 随之入 wire）。楼里的文件是楼的记忆，服务端在被问的那一刻读盘——**文件是权威**，另存一份索引就是第二个权威。`QUERY_NAMES` 因此从 10 增到 11，schema 哈希随之从 `238f11b2…` 变为 `85705c03…`：客户端与服务端同批发布，旧页面会在握手期被明确拒绝并提示刷新。

**R1.10 改：`Welcome` 携 `city: Option<Address>`，`decide_frame` 增一个 `city` 入参。** 事件流只送连接之后发生的事，而城市的名字写在 Ledger 的第一条记录里——一个今天打开的浏览器永远等不到它。握手是「这是哪座城」的自然回答处；服务端从同一条创世记录读它，故两边不构成第二个权威。同批：`init` 把城市名写进创世记录的 `addr`（此前是 `None`，城市名只活在目录项里）。

**R1.06 改：`ApprovalsAnswer.items` 携 `kernel::ApprovalItem` 全项，`ApprovalSummary` 删除。** 旧摘要类型丢掉了 `cluster_key` 与 `created`，于是界面无法按类聚合、也排不出「谁等得最久」；服务端为了填它还要从事件载荷里猜一个 `summary` 字段——那个字段从来没被写过，故每一条待批项都渲染成「(no summary recorded)」。载荷本身就是 `ApprovalItem` 的序列化，原样送过去既少一次有损转换，也让「什么算一类」只有 `web::approval::inbox` 一处答案。

- **为什么是强类型答面而不是一团 `Payload`**：`web` 只依赖本 crate，故发帧的边界 crate 欠对方一套读帧的词汇（同 kernel 再导出的理由）。一个无类型载荷会把解析责任推给每一个视图模块，每一个都得自己猜一遍形状。
- **`Answer::Unavailable { query }` 是一个真答案**：本期不求值的视图报自己的名字，而不是返回空结果——空城与未实现在界面上必须长得不一样。
- **`CityAnswer.buildings: Vec<BuildingProgress>`（P1.04）**：每栋楼一行，携 `Progress` 与 `problems`。解析不出的行进 `problems` 并照显——悄悄丢掉读不懂的行，等于按一个没人选过的分母报进度。
- **五维成本携权威总额**：`CostAnswer.total` 与五个维度各自求和相等；界面按 `total` 算占比而不自己归一，未归因余额因此看得见。

- **採用 `broadcast` 而非每连接一个队列**：多个标签页是常态；慢客户端被 `Lagged` 拉下而不拖住写入方，它重连时从 `Welcome.resume_from` 补齐（P1.05 接前端时兑现）。

## 8.5 两个设计

**握手的失败处置：断连 vs 降级协商。** 取断连。理由是这条错配的真实来源早已写明——「浏览器可能缓存了旧前端而服务端已经升级」，而降级协商要求服务端同时维护两套 wire 语义，那是两个权威。断连＋提示刷新把一个协议问题还原成一个刷新动作。**被否**：版本协商（多版本共存）——它的成本在每次改 wire 时都要付，而收益只在「用户不肯刷新」这一个场景里兑现。

**上传通路：WebSocket 分帧 vs 独立 HTTP 端点。** 取独立 HTTP 端点。理由是「WebSocket 不适合携带数百 MB 的帧」。**被否**：在 WS 上自制分片协议——那是重新实现 HTTP 已经做好的事（范围请求、断点续传、进度），且会让 `Attach` 的传输失败与命令失败混在同一条通路上难以区分。

**P1.11 增：`POST /enroll`，唯一携凭证字节的路由**

它是 HTTP 而非 socket 帧，因为 socket 的 `WireCommand` **拼不出** `PutSecret`（`NoSecret` 无值）。两半合起来才是完整保证：类型层管住帧，`decide_enroll` 管住字节——因为字节总可以被 POST 到一个路由上。

- **只认回环对端，配对令牌也不算数**：令牌认的是人，而这条规则管的是**字节走到哪里**。拒绝的第三段指向宙主机，于是它是约束而非死路。
- 壳里零策略：判定在 `decide_enroll`，壳只搬字节——同 `decide_bind`／`decide_frame` 的切法，故无需跑服务即可穷尽测。
- 应答返回 `secret:<realm>/<name>`；值不回声、不入事件载荷。入金库由 `Sealed::into_vault_value`（住 kernel::secret，即 expose 白名单三文件之一）完成，开封因此**不发生在装配层**。

**R1.14 增：`Login` 携 `LoginStep { Begin, Code { code } }`，WIRE_V 1→2**。两步之间站着一个人：provider 在它自己的页面上把 code 显示给他，他再带回来。**用穷尽枚举而不是 `Option<String>`**——「开始登录」与「兑付这个 code」是两个动作、两种失败，一个页面表达其一时恒不该被读作另一个。`COMMAND_NAMES` 不变，故 schema 哈希单靠名字表不会动；这正是 `WIRE_V` 存在的那种情形（语法换形而名字没换），于是版本进位、旧页面在握手期被明确拒绝。**恒不开回环监听端口**：该 provider 的 redirect 就是它自己的页面，多一个监听口就是多一条没人走的入口。

**R1.14 增：`Login` 携穷尽枚举 `LoginStep { Begin, Code { code } }`，WIRE_V 1→2**。两步之间站着一个人：provider 在它自己的页面上把 code 显示给他，他再带回来。用枚举而不是 `Option<String>`——「开始登录」与「兑付这个 code」是两个动作、两种失败。`COMMAND_NAMES` 未变故 schema 哈希单靠名字表不会动，这正是 `WIRE_V` 存在的那种情形。**恒不开回环监听端口**：该 provider 的 redirect 就是它自己的页面。

**R1.16 增：五个查询各有自己的答**（`InboxView`／`DiscardView`／`RegistryView`／`ArchiveSearch`／`Metrics`），WIRE_V 2→3。此前它们一律答 `Unavailable`，界面据此什么都画不出来。三条口径：①**队列折叠着看不消费着看**（`Inbox::pull` 要拿走才给内容，看一眼就取走的视图会改变它所报告的对象）；②归档在被问的那一刻读盘（同 `BuildingView`，文件是权威）；③**`Metrics` 恒不携钱**——钱是 `CostView` 的，一个数字两个主人就是两个数字开始互相矛盾的起点。

**F1.01 改：`DiscardLine.restoration` 携 `Option<Restoration>` 而非一个句子，WIRE_V 3→4**。回收站那一行的「怎么拿回来」原本在服务端被拼成 `"tracked: file:…"`，而客户端早已持有它的唯一措辞处（`web::approval::ReturnPath::sentence`）——**一件事两个渲染权威**，而服务端那个还拼不出可执行的那句话。现在计划以它自己的形状上线（载荷本来就是 `Restoration` 序列化出来的，故读得回去）；`None` 的意思是**这一条记录用了本构建读不懂的方案**，界面据此画一行而不给动作（`ReturnPath::Undescribed`）——行恒不隐藏，因为藏起一件被删的东西比承认读不懂它的方案更糟。`QUERY_NAMES` 与 `COMMAND_NAMES` 未动，故哈希只因 `WIRE_V` 而变——又一例「语法换形而名字没换」。

**R1.18 增：`POST /acp` 与 `AcpSink`**。外来编辑器的请求走自己的路由，不挤 Command 面：它自带鉴权、要一个当场的回答，而 Command 面的回答是事件流。三条口径：①**令牌在本 crate 判**（配对令牌住这里，常数时间比对也就住这里），只把 `authentic` 一位传进去——拒词由 `protocol::admit` 措辞，「未配对者只学到一位」因此只有一个权威；②回给编辑器的只有 `AcpProgress` 三字段，run id 是工人接单时才铸的，故受理那一刻诚实的答案是「已受理、未完成」；③没配对令牌的城即回环独占，与 control surface 同一条规矩。

**P1.11 增：三帧登记面**（§8-1 golden 同集更新）——`AttachEndpoint`（人刚输入的 URL＋兼容格式＋`secret:` 引用；**引用有字节形，凭证没有**）、`SelectModel`（标签→模型＋两个探不到的 token 数）、`EndpointView`（设置页的读；`EndpointsAnswer` 里 `has_credential` 是关于凭证能回答的全部）。

**P1.12 增：三个 kernel 类型的再导出**（`DialectKind`／`Effort`／`ModelTag`）。`web` 只依赖 `channels`（拓扑图），而设置页要拼写这三个词；再导出而非镜像定义，因为镜像就是同一规则的第二个权威——同 §8-0 对 `Mode` 的口径。

### 8-3 channels::auth（S4.03；形状 2 值类型 ＋ 形状 1 判定）

```rust
pub struct PairingToken(B3Hash);               // 只是摘要，不持明文
impl PairingToken {
    pub fn mint(entropy: [u8; 32]) -> (Self, String);  // 右侧即只展示一次的配对码
    pub fn from_configured(raw: &str) -> Result<Self, AxError>;
    pub fn digest(&self) -> B3Hash;                    // 交给 ServeConfig 的全部
}
pub fn verify(presented: Option<&str>, expected: &B3Hash) -> bool;  // 常数时间
```

四条决定：

（a）**熵入参不采样**——使铸造可重演、可测，与「种子 RNG 单点发放」一致。

（b）**`PairingToken` 不持明文**（施工中由 `xtask secret` 门逆推出的修正）。初版写的是 `Sealed<String>` 加一个 `display_form()` 里的 `.expose()`，门咬住。候选的应对是把该点加进兑付点白名单——那是修门。**实际修的是因**：本模块真的不需要明文，`mint` 把配对码直接交给调用方去展示，自己只留摘要。封一个值再在下一行解封是表演；**根本不持才是我们想要的性质**。

（c）**字母表剔除可混淆字符**（0／O／1／l／I／5／S），29 符号×四组五位≈ 97 位熵。理由不是审美：配对码要被人读出来、在另一台机器上手敲进去，一个口述会错的码，代价由用户在另一台机器前承担。

（d）**分组形兼顾了 secret 门**：每片 5 字节远低于 20 字节阈值，测试里的配对码字面量不会被熵侦测器咬住（Handoff 坑 5）。

### 8-4 channels::control（S4.03；形状 1 判定函数）

```rust
pub enum Intervention { Steer, Cancel, Takeover, Rollback, Halt, Release }
pub enum ControlVerdict {
    Intervene { verb: Intervention, run: Option<RunId>, must_write_handoff: bool },
    NotAnIntervention,
    Refuse(AxError),
}
pub fn classify(command: &Command) -> ControlVerdict;
```

**本模块持有的唯一规则**：中断一个活着的 Run 的三个动词（Steer／Cancel／Rollback）**恒以 Handoff 收尾**，使下一位（人或 Agent）拿得到完整现场。`Takeover` 同理。`Halt`／`Release` 按 scope 停一片，不针对单个 Run，故 `run` 为 `None` 且不强制 Handoff。

**`must_write_handoff` 为什么是返回值而不是副作用**：本 crate 不持 Ledger 句柄（§7 已写）。它只能**说出义务**，履行义务的是装配层。把它做成返回值的代价是装配层可能忽略它——故 S4.03 同卡交付一条断言：一次干预的事件序里若无 `handoff_written`，即失败。

**为什么不并入 `server`**：`server` 的职责是「这个字节流能不能变成一个帧」，`control` 的职责是「这个帧是不是干预、干预要留下什么」。后者在无网络的 citysim 里也成立，前者不。两个职责的变化率也不同：加一个动词不应该碰监听器。

### 8-5 channels::aggregate（S4.04；形状 1 判定 ＋ 形状 2 值类型）

```rust
pub struct CityLabel(String);                     // parse 拒空与控制字符
pub struct Upstream { label, address: String, token_digest: Option<B3Hash> }
pub struct Sighting { city: CityLabel, event: EventRecord }
pub struct Forwarded { address: String, query: Query }   // 无 Command 字段
pub struct Aggregate { /* BTreeMap<CityLabel, Upstream> */ }
impl Aggregate {
    pub fn attach(&mut self, Upstream);  pub fn detach(&mut self, &CityLabel) -> bool;
    pub fn cities(&self) -> impl Iterator<Item = &Upstream>;
    pub fn ask(&self, &CityLabel, Query) -> Result<Forwarded, AxError>;  // 唯一发送面
    pub fn merge(Vec<(CityLabel, Vec<EventRecord>)>) -> Vec<Sighting>;
}
```

**硬约束的存放形式**：它只有一句「聚合层只转发 Query 与 Event，恒不转发 Command」。它在这里**不是一个判断**，是一个缺席：`ask` 接 `Query`，而没有第二个发送方法。连 `Forwarded` 也不带 Command 字段，使下游无处升格。理由：一个可代发命令的聚合层是**不在任何一本账上的跨城权威**——目标城无法把命令与发起人对应。

**合流序为什么不能用 seq**：两座 City 各有各的 Ledger，各自从 1 编号。故排序键取 `(t, city, seq)`：时间先行，label 入键而非做最后的破平——因为两城同一毫秒是常态，而合流视图必须两次一样。

**本模块不含传输**：需要确定性与可测性的是合流序与转发面，两者都不需要 socket。实际连接属装配层（S4.05 界面接入时）。

### 8-6 crate 面的两项（S4.05 随 `web` 接入时定下）

**一、`server` feature**（默认开）。`web → channels` 是 depmap 冻结边，而 tokio 的 mio **编译不到 wasm32**。故监听器进 feature：`server = ["dep:tokio", "dep:axum"]`，`web` 取 `default-features = false`，只得 wire／control／aggregate 三模块。浏览器里本来也没有可供监听的 socket，这条分割与现实同形。

**被否**：把 wire 拆成第六个 crate。那要改 ARCHITECTURE §2 冻结拓扑（需 verdict），而 feature 边界已足以表达「词汇与监听器分开」这一件事。

**代价与看守**：`cargo clippy --workspace --all-features` 在 host 上恒开 `server`，故关掉它的构建不在 `just check` 覆盖面内；补以 `just check-web`（wasm32 目标上跑 clippy，路径上必然关掉 `server`）。

**二、kernel 类型再导出**。本 crate 的公开签名上出现的 kernel 类型（`EventRecord`、`AxError`、`RunId`、`Seq`、`Address`、`BudgetCap` 等）一律从 `lib.rs` 再导出。理由是拓扑硬约束：`web` 只能依赖 `channels`，一个拿不到 `EventRecord` 的客户端读不了自己收到的帧——**发帧的边界 crate 欠对方一套读帧的词汇**（C-REEXPORT）。

**被否**：给 depmap 加 `web: channels, kernel`。那是改冻结面去适应一个本就有标准解法的问题。

**再导出集随 `web` 的需要生长**，当前含：标识与度量（`Address`、`RunId`、`Seq`、`TimeMs`、`UsdMicros`、`Tokens`、`B3Hash`、`GitOid`、`IdemKey`）｜事件（`EventRecord`、`EventDraft`、`EventKind`、`Payload`）｜判定结果（`AxError`、`AxCode`、`Progress`与两系、`BudgetCap`、`BudgetUse`、`PolicyVerdict`、`Autonomy`）｜待批与删除（`ApprovalItem`、`ApprovalId`、`ApprovalClass`、`ApprovalSource`、`ClusterKey`、`Restoration`、`Locator`）。

> **施工纪律（本期被 apisync 门咬两次后写下）**：动本 crate 的再导出列表就是动公开面。本节必须与该变更**同一提交**更新——它很容易被当成「只是多写一行 `pub use`」而漏掉，而门不接受这个理由。

### 8-7 一次会话有名字，而名字就是它干活的那个房间（F2.11；未实现，设计已定）

> **状态：设计落定，代码未写。** 下一个会话从这里开工；先写失败测试，再实现。

```rust
pub const WIRE_V: u32 = 5;                     // 4 → 5：一个字段，一次握手拒绝
WireCommand::Dispatch { addr, task, goal, mode, budget, idem,
                        session: Option<SessionName> }
pub struct SessionName(String);                // 形状 2；一个构造点，内容即一个地址段
```

**问题不在线格式上，在于没有人给新会话开房间。** ARCHITECTURE §6 自己写着 `JOB.md — the task for this session`：房间本来就是会话的工作区。今天派活要人手打一个 `building/room`，于是所有派活撞进同一个地址，模板文件互相覆盖。

- **不加第四层**：每个 Run 一个目录会切断 Handoff 的连续性，而连续性正是一个 session 之所以是 session 的东西；`collab` 整套（draft／PR／fanin）也都建立在「几个居民在同一栋楼里不互相踩」上。
- **地址给楼，名字给会话**：`addr` 可以只是一栋楼；`session` 在它底下开一个房间。重名加数字后缀（`refactor`、`refactor-2`），而不是拒绝——一个人连着开两次同名会话是常事。
- **向已有房间派活即继续那条会话**：它的 `Handoff.md` 与 `JOB.md` 就是连续性。「回复某个 session」因此不需要新概念。
- **`RunId` 不变**：账上的身份仍是 `b3(job|addr|now)`；名字是房间的标签，不是 Run 的。一个房间一生中的多次 Run 合起来才是一条会话。
- **`SessionName` 是值类型而非 `String`**：它必须能当一个地址段（无 `/`、无 `.`／`..`、无控制字符、非空、不叫 `.sprawling`），否则一个人输入的字会变成一条路径。构造点一个，拒绝携 recovery。
- **`WIRE_V` 4 → 5**：握手处的 schema hash 因此变，旧页面拒绝而不是误读——这正是那个机制存在的理由。
- **服务端开房间落在 `city`**（新一节，同期写）：已存在名字→加后缀，目录建在楼下；`.sprawling` 不得为会话名（F2.08 的谓词直接答这件事）。
- **客户端（同期写在 web-SPEC）**：派活条第一格从「你猜 building/room」变成「选一栋楼 ＋ 这次叫什么」；直播页与楼页显示名字而不是 `short_run` 的十六进制。

## 9 工作流程

（随 S4.02 填：从监听、绑定面判定、握手、鉴权，到帧解码、处理器分派、Event 推送的完整通路。）

## 10 实现逻辑

（随 S4.02–S4.04 逐卡填。）

## 11 边界枚举

（随卡填：握手哈希不配／令牌错／绑定非回环无令牌／上传中断／客户端半关连接／聚合上游断线／Event 推送背压。）

## 12 错误处理（逐码答「能否定义掉」）

- **`E_WIRE_MISMATCH`**：不可——它是装载期五码之一（封闭白名单），且它的存在理由就是「浏览器缓存旧前端」这一 WebUI 特有错配。类型无法定义掉跨版本的字节。
- **`E_CONFIG_INVALID`**：不可——绑定非回环而无令牌必须在**启动时**拒绝，这是配置判定不是请求判定。
- **`E_SIGNAL_UNKNOWN`**（ARCHITECTURE §11 点名的三条待消解之一，本 crate 须作答）：**部分定义掉**。形状未知的一半可以定义掉——握手的 schema 哈希保证同一连接的两端共享同一份 Signal 枚举，故「收到一个不认识的 Signal 种类」在单个连接内不可表示。语义未知的一半不可定义掉——一个 Resident 收到语法合法但自己不处理的 Signal 类别，这是真实结局，判定位置在消费端 `collab::inbox`（P2），channels 只是 `Attach{notify}` 的产地。**结论：本码不在 channels 消解，其生产消费者仍待 P2；本条即 ARCHITECTURE §11 要求的作答，不是默认保留。**

## 13 依赖选型

| 依赖 | 用途 | 判据与替代 |
|---|---|---|
| `tokio` | 异步运行时；本 crate 是全库首个真异步消费者 | B.7 Stage 0–1 行已钉，S3 明确推迟至此。替代（自写 reactor）被 C12 与「复用既有机制」双拒 |
| `axum` 0.8.x | HTTP 静态资源、上传端点、WS 升级 | B.7 写「axum 或同类」。取 axum：tokio 官方序列、7978 下游 crate、2026-04-14 仍在发版，且其 WS 支持内含 tokio-tungstenite，省掉一层版本对齐 |
| `tokio-tungstenite` | WS 协议 | 经 axum 传递依赖（axum 0.8.9 已升至 0.29）；**不直接依赖**，避免两处版本权威 |
| `serde`／`serde_json` | 帧编码 | 已在 workspace |
| `kernel` | AxError／EventRecord／IdemKey／Sealed／Address | 唯一上游 |

**不引**：任何通用 RPC 框架（wire 是 17＋9 个具名 variant，不是一个可扩展的服务定义）；任何 session 中间件（鉴权面只有配对令牌一件）；任何穿透／中继库。

**依赖面代价须实测并回填**：引 tokio＋axum 后 `channels` 的依赖 crate 数，按 S3.12 的先例（wasmtime 使 runtime 从 71 涨到 257）在本卡收口时记录。

## 14 硬编码声明

- **默认绑定地址恒为回环**——它不是配置的默认值那么软，而是「非回环需要额外条件才允许」的判定起点。
- **schema 哈希的派生框架**（哪些类型入哈希、以什么序）一旦定下即是冻结面：改框架＝旧客户端全部拒配。故派生框架与 `IDEM_DERIVE_V` 同规，携版本字节。

## 15 影响面

- 根 `Cargo.toml`：`workspace.dependencies` 增 tokio／axum（guard 辖区，须 `Verdict:` 尾注）。
- `crates/sprawling/assembly.rs`：`serve` 装配点——gateway 的 Custodian 生产装配、memory 三视图的界面查询、`runtime::replay` 补写面的启动扫描都在此接线（ARCHITECTURE §6 接线台账 S4 到期项）。
- `xtask/api-baselines/channels.txt`：本期起算。
- ARCHITECTURE §6 模块表 channels 五行：随各卡从「未建」翻「已建」。
- `crates/web/web-SPEC.md`：wire 的 Command／Query／Event 表是 web 的唯一上游，两份 SPEC 必须一致。

## 16 测试与约束

- 计数断言：Command 17、Query 9，逐名对本 SPEC §8-1 两表（与 kernel 的 specalign 同规——**若 S4.02 后 wire 表也值得机器看守，评估扩 specalign 覆盖面**）。
- trybuild 两反例：远程 `PutSecret`（含 `Sealed` 的帧不可序列化）；`aggregate` 发 Command（发送面无该入口）。
- 常数时间比较的性质测试：差异位置不影响比较耗时。
- 握手 golden：schema 哈希入快照；改 wire 类型必须同时改快照与本 SPEC。
- 绑定面判定的单元测试：回环／非回环×有令牌／无令牌四格，只有「非回环＋无令牌」拒绝启动。
- 约束：非测试代码遵守 C3 硬化全条；全库禁裸 spawn（确定性第 3 条）——**本 crate 的并发必须是结构化的，带取消令牌**，这是引入 tokio 后第一条要守住的线。

## 17 模型体验

零字节，因为 wire 面是人与服务端之间的协议，不进入任何 prefix。间接影响有一条：`Steer` 经 control surface 送达后，落点是**结果信封**（追加在下一次工具调用结果末尾，前缀 `user`），那几个字节由 `runtime::pipeline` 计入，不由本 crate 计入。

## 18 文档同步

- ARCHITECTURE §6 模块表 channels 五行状态；§6 接线台账中 S4 到期的五行（channels 全部、gateway Custodian 生产装配、memory 三视图界面消费者、attribution→CostView、replay 补写面→resume 启动扫描）。
- AGENTS.md：若新增命令面配方则同步命令表。
- 依赖钉版表「axum 或同类 / tokio-tungstenite」行：回填实际选定版本。
- `crates/sprawling/sprawling-SPEC.md`：`serve` 子命令的装配面。

### 8-8 第三类帧：模型还在说的时候（V3.13；形状 2 值类型）

**一个 token 增量不是效果，所以它不是事件。** 事件是发生过的事：有序号、进账本、可重放、可离线验。增量没有序号、永不落盘、无法重放，而且一个客户端漏掉一条什么也没丢。把它折进事件流，等于给「模型说了什么」造第二份、不可验证的历史——而这座城全部的主张就是那份历史可验。

于是它是 `ServerFrame` 的第四个取值：

```rust
pub enum ServerFrame { Welcome(..), Event(..), Answer(..), Refusal(..), Delta(Delta) }
pub struct Delta { pub run: RunId, pub text: String }
```

`WIRE_V` 11 → 12，schema 哈希随之变（`ServerFrame` 不进名字表，故这是「语法换形而名字没换」那一类，版本进位、旧页面在握手期被明确拒绝）。

**三条口径，各自都是承重的：**

1. **携 `RunId` 而不携序号。** 客户端据此把缓冲挂在一个 run 名下，并在该 run 的 `model_returned` 到达时整个丢掉。这就是「结算文本赢」的全部机制——一条断言钉住它（`web::session`）。
2. **两条广播通道而不是一条。** 增量与事件的丢弃语义相反：漏掉的增量什么都不是，漏掉的事件是必须从账本补回的历史。共用一条通道会让一次话多的模型把记录挤出慢读者的窗口。
3. **没人看时不开流。** 装配层只在有客户端时装 sink；`RunHooks.deltas` 为 `None` 的 run 走原本的阻塞调用，字节不差。于是 citysim 与离线重放的路径一字未改。

**服务端半边（V3.12）在 `gateway`：** `kernel::Model` 多一个 `call_streaming(req, onto)`，默认实现就是 `call` 并且不报告任何增量——一个没有流的适配器因此是诚实的而不是坏的。`gateway::endpoint` 覆盖它：请求带 `stream: true`，逐行读 SSE，`dialect::increment_of` 只认各 dialect 用于助手散文的那个字段（工具参数与 thinking 块一律不报——半个工具参数不是短一点的工具参数），最后 `dialect::settled_from_stream` 把帧重装成**非流式的那个形状**，交给同一个 `response_from_wire`。**结算答案因此只有一个解析器**：流式调用与阻塞调用不可能对同一个回复得出两个结论。流被切断仍然表现为读取错误，永不表现为一个变短的回答。
