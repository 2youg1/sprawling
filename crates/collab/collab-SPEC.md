# collab-SPEC.md

> crate：`collab`。本 SPEC 先于代码存在；实现不多不少地遵守本文。
> 骨架：apostle-sdd 十七节；按模块分章、每章自足（ARCHITECTURE.md §5）。
> 动手前先读所用工具与依赖的**官方文档或官方 agent 指南**，再写本文的接口节。

## 1 需求拆解

本 crate 是「两个 Agent 在同一栋楼里干活而不互相踩」的机械层。七个模块拆成五张卡，每张卡落地时先补齐本文对应章节（接口先行）：

| 卡 | 模块 | 这张卡回答的问题 |
|---|---|---|
| P2.04 | `inbox`、`steer` | 一条消息怎么从一个 Agent 到另一个，而重复投递不会变成重复副作用 |
| P2.05 | `draft` | 两个 Agent 写同一份东西时，谁手里拿着它 |
| P2.06 | `workshop`、`fanin` | 一件活拆成多个节点后，谁按什么序跑、结果怎么收回来 |
| P2.07 | `pr` | 写代码的人不验自己的代码，这件事由什么强制 |
| P2.08 | `arbiter` | 两个 Agent 不同意时，升到哪里（§8-7） |
| P3.01 | `signal_tool`、`goal_tool` | 上面六个机制怎么变成 Agent 手里真能调的东西（§8-8、§8-9） |
| P3.02 | `pr_tool` | 开 PR 与 worktree 为根的 Run（§8-10） |
| P3.07 | `triage` | 一条外来信号该送到哪个 Address（§8-11） |

## 2 验收标准

逐卡写在 ARCHITECTURE.md §10 的收口栏，本文在卡落地时把它展开成断言名。**本节现在空着是事实而非疏漏**：一个未施工模块的验收标准写在接口存在之前，只会在施工时被改掉。

## 3 假设与歧义

一层深（delegate 值上无 delegate 方法）已在 `kernel::delegation` 定谳，本 crate 只把它当作前提，不重议它。

## 4 现状分析

`collab` 自 S0 起是空壳（只有 `lib.rs` 的 crate 文档）。它要消费的 kernel 判定面已建已测而仍无生产消费者：`kernel::goal`（同资源相斥）、`kernel::repair`（repair lease）、`kernel::delegation`（delegate 两类）。接线台账（ARCHITECTURE.md §6 末）把它们的接线期记在 P2，本 crate 就是那个消费者。

## 5 权威信源

「多 Agent」的语义（一层深、干预五动词、**实现者不自测**、为什么赌多 Agent 的六条及其判负条件）；ARCHITECTURE.md §12 模块图的 collab 段与 §9 七形状；`kernel-SPEC.md` 的 goal／repair／delegation 章。

## 6 命名统一

Signal｜Inbox｜Steer｜HeldDraft｜hold token｜four-way return｜Workshop｜NodeContract｜fan-in｜Artifact｜arbitration｜Triage。概念名一律英文原词；该用什么词见 `docs/glossary.md`，不该用什么词见 `xtask/lexicon.toml`。

## 7 模块边界

**三件邻居的活，及它们各自的主人**（写「X 归 Y」而非「不做 X」：前者告诉施工者去哪，后者只告诉他别去哪里）：

- **物理隔离归 `memory::worktree`**（P2.03）：本 crate 决定谁干什么、谁拿着哪份草稿；一节点一棵树与磁盘上限归 memory。
- **人的五动词归 `channels::control`**：`Steer`／`Cancel`／`Takeover`／`Rollback`／`Halt` 从人那侧进城已有入口；本 crate 的 `steer` 只管 **Agent 发给 Agent** 那一条通道——两条通道入口不同而落点相同。
- **处置与监护归 `runtime::watchdog`**：停滞判据与纠正→冻结的升级梯已在那里，本 crate 不建第二套。

**L2 工具为什么住在本 crate（P3.01 定谳）**：一件工具是 `kernel::tool` 缝的适配器，而适配器必须能命名它暴露的机制。依赖法写着 `runtime: kernel, memory, gateway`（ARCHITECTURE.md §2）——**runtime 恒不得指名 collab**，所以 `Signal` 与 `GoalEntry` 的工具面拼不进 `runtime::tools/`；放进 bin 则把四百行判定塞进全图最脏的那个文件且 citysim 测不到。故 L0 三件在 runtime，L2 协作三件在 collab；§3 缝清单的「生产适配器」列同卡加一项。

## 8 接口先行

逐卡写：每个模块落地前先在本节开一个 `### 8-n <模块>（卡号；形状）` 子节，给出类型签名与它们为什么是这个形状，写法同 `city-SPEC.md` §8。

### 8-1 collab::inbox（P2.04；形状 2 值类型＋形状 7 投影）

```rust
pub struct SignalId(String);                       // 非空、无空白；去重的依据
pub enum SignalKind { Mention, Thread, Broadcast, Steer }
pub enum Lane { Urgent, Ordinary }                 // 由 kind 推出，不由调用方给
pub struct Signal { /* id、kind、from、room、room_version、payload、at —— 私有 */ }
impl Signal {
    pub fn new(id: SignalId, kind: SignalKind, from: String, room: Address,
               room_version: Version, payload: Payload, at: TimeMs) -> Result<Signal, AxError>;
    pub fn lane(&self) -> Lane;
    pub fn enqueued_payload(&self) -> Result<Payload, AxError>;   // signal_enqueued
    pub fn consumed_payload(&self) -> Result<Payload, AxError>;   // signal_consumed
}
pub struct Inbox { /* 两条 memory::EventQueue＋bandwidth —— 私有 */ }
impl Inbox {
    pub fn new(capacity: u64, bandwidth: u32) -> Inbox;
    pub fn deliver(&mut self, signal: &Signal) -> Result<Admission, AxError>;
    pub fn pull(&mut self) -> Result<Vec<Signal>, AxError>;   // ≤ bandwidth，急件先出
    pub fn take_steer(&mut self) -> Option<Signal>;           // P3.07：只取急件 lane 一件
    pub fn pending(&self) -> u32;                             // status 的 signals_pending
}
```

- **`take_steer` 取的是 lane 而不是 kind（P3.07）**：急件 lane 与 Steer 是同一个集合（`lane()` 只把 Steer 送进去），故“取一件能插队的东西”不需要在队列之上再建一层 kind 筛选——第二处判定只会与 `lane()` 分叉。它不碰普通 lane，因为插队与拆信是两件事：前者是别人把话塞进正在干活的人手里，后者是它自己决定去看信箱。
- **读不回来的载荷在这条路上丢弃而不报错**，并写进了契约：调用点是一次 Run 的安全点，在那里除了“继续”的唯一替代选项是为别人的一条损坏条目停掉这一跑；同一条载荷仍然会在 `pull`（模型自己那扇门）上大声报错，所以事实不会消失。

- **去重先于副作用**：去重由 `memory::EventQueue` 的 `seen` 给（IdemKey 由 `SignalId` 派生），而不在本模块再建一张表——一条规则一个权威。此事要成立，同一个 id 就必须恒落同一条 lane，**所以 lane 由 kind 推出、不由调用方给**。
- **`SignalKind` 四值而非三值**（早先的设计记三值）：紧急与否必须是 Signal 自己的属性，否则同一件 Signal 从两个调用点进来会落入两条 lane，去重就有了两个权威。
- **插队首＝一条先被排干的 lane**，不是队内优先级字段：一个结构里共存两种顺序，就会有人读错其中一种。
- **pull bandwidth 在接收方**：发送方推不动接收方的上下文窗口；一次 `pull` 最多取 bandwidth 件，Signal 在 prefix 里恒占零字节，常驻的只是 `status` 的 `signals_pending`。
- **洪水交给 backpressure**：`deliver` 返回 `kernel::Admission`，削峰判定住 `kernel::backpressure`，计数住队列；本模块不自定义第二套限流。
- **`E_SIGNAL_UNKNOWN` 已定义掉**（定谳三码之一；P3.01 执行）：本模块自写自读载荷，kind 是穷尽枚举，一个本版本不认的 kind 只能来自更新的二进制写的 Ledger，而那已由版本方向门（`E_LOG_VERSION_UNSUPPORTED`）拒在外面；同一句话里不认的 kind 在本版本写入面也拼不出来（`SignalKind::parse` 拒它，报 `E_INVALID_ARGS`）。实测佐证：删除前全仓只有 `kernel::error` 自己提到它，零生产者。实施：删 `AxCode::SignalUnknown`，AxCode 36 → 35，kernel-SPEC §8-1 表同集删行。
- **`Signal::from_payload` 是 `enqueued_payload` 的逆**（P3.01 新增公开面）：没有它，投影重建就要在仓库里长出第二份 Signal 解析器。重建方式是**先筛后送**：从 Ledger 收齐 `signal_enqueued` 与 `signal_consumed` 两组 id，只把未被消费的按原序 `deliver` 一遍——于是队列不需要「按 id 删除」这个不属于队列的动作。

### 8-2 collab::steer（P2.04；形状 2 值类型）

```rust
pub struct Steer { /* source、text —— 私有 */ }        // 落点：追在下一次工具结果末尾的一段
impl Steer {
    pub fn from_person(text: &str) -> Result<Steer, AxError>;   // 唯一能写出 `user` 前缀的构造子
    pub fn from_signal(signal: &Signal) -> Result<Steer, AxError>;
    pub fn source(&self) -> &str;  pub fn text(&self) -> &str;
}
pub struct AgentSteer { /* id、text —— 私有 */ }
impl AgentSteer {
    pub fn new(id: &str, text: &str) -> Result<AgentSteer, AxError>;
    pub fn signal(&self, id: SignalId, room: Address, room_version: Version, at: TimeMs)
        -> Result<Signal, AxError>;                              // Agent 侧只能走 Inbox
    pub fn landing(&self) -> Steer;                              // `@id`
}
```

- **两个入口、一个落点**：人的 Steer 只从 control surface 进城，恒不走 Inbox；Agent 的 Steer 是一件插队首的 Signal。两者都追在下一次工具结果末尾，因为模型只需要认识一种形状。
- **`user` 前缀只有一个构造子写得出**：`AgentSteer` 的 source 由它自己的 id 拼成 `@id`，故一件自称来自人的注入内容拼不出 `user`——入口分立是安全要求，类型把它变成判定。
- **Steer 不打断动作**（下接 8-3）：它在安全点被消费并推进（`runtime::turn` 已定）；同一边界上 Cancel 压过 Steer，因为停是不可撤销的那个。本模块只产出落点形状，不重建中断梯。
- **本模块自 P2.04 建成起零调用方，P3.07 才把线接上**：装配层的 `interrupt_for` 只读人的命令队列，于是 `Steer::from_signal` 与整个 `AgentSteer` 是一套写好、测过、永远不会发生的机制。现在中断源先问人、再问本屋信箱（`SignalDesk::take_steer`），**人压过居民**。
- **属名就是回信地址，这是 `@id` 不能改成别的什么的理由**：模型在窗口里读到 `@market/hana:` 时，它读到的既是“这句话不是人说的”，也是 `signal` 的 `to` 参数该填什么。一个只标注“来自另一个 agent”而不给地址的前缀，会让回信变成猜测。

### 8-3 collab::draft（P2.05；形状 1 判定＋形状 2 值类型）

```rust
pub struct Draft { /* author、room、seen: Version、body: Payload —— 私有 */ }
pub enum Return { Rewrite, SendAsIs, Withdraw, ForceInformed }   // 四路退回，穷尽
pub struct HoldToken { /* room_version、turn —— 私有；只能由服务端发 */ }
pub enum Submission { Delivered, Held { token: HoldToken, holds: u32 }, Escalated { holds: u32 } }
pub enum Resolution { Delivered, Withdrawn, Held { token: HoldToken, holds: u32 },
                      Escalated { holds: u32 }, TokenVoid { token: HoldToken } }
pub struct Drafts { /* 逐 (author, room) 的 token 与连续退回计数 —— 私有 */ }
impl Drafts {
    pub fn submit(&mut self, draft: &Draft, current: Version, turn: u32) -> Submission;
    pub fn resolve(&mut self, draft: &Draft, choice: Return, current: Version, turn: u32) -> Resolution;
    pub fn held_payload(&self, draft: &Draft, current: Version) -> Result<Payload, AxError>;
    pub fn resolved_payload(&self, draft: &Draft, choice: Return) -> Result<Payload, AxError>;
}
```

- **`room_version` 是通信侧的乐观并发**，与文件写入的 `base_version` 同型：发言携着自己所见的版本，冲突因此显式。
- **四路都摆出来，不默认重写**：「Room 变了」不等于「这条发言作废」，判断权归发言者。机制在 prefix 零常驻，被撞回的那一刻才学。
- **`ForceInformed` 不是免费的**：它消费一枚服务端发的 hold token，而 token 绑着退回当时的 `room_version`；Room 又往前走一步，token 即作废并重新退回。**一般规律**：协调闸门上的旁路开关必须是对服务端已展示状态的确认，不能是客户端的一个意见——无条件生效的旁路参数，会被一个被要求「高效」的模型学会预防性地带上，闸门于是在没有任何人决定废除它的情况下静默地不再存在。
- **token 本回合内有效**：过期不采时钟，而是比回合号——时间只入参不采样（确定性二）。
- **连续退回 ≥ `DRAFT_HELD_ESCALATE` 即升 owner verdict**：两个 Agent 互相撞回的活锁不在退回循环里空烧。

### 8-4 collab::workshop（P2.06；形状 2 值类型＋形状 1 判定）

```rust
pub struct NodeId(String);
pub struct NodeContract { /* id、goal、depends_on、reads、write_domain、owner、done_check、budget、stop */ }
impl NodeContract { pub fn job_text(&self) -> String; /* 落盘即该节点的 JOB.md */ }
pub struct Workshop { /* BTreeMap<NodeId, NodeContract> —— 私有 */ }
impl Workshop {
    pub fn new(contracts: Vec<NodeContract>) -> Result<Workshop, AxError>;  // 重名／悬空依赖／环，三者在构造点拒
    pub fn schedule(&self) -> Vec<NodeId>;                                  // 确定性：同图同序
    pub fn ready(&self, done: &BTreeSet<NodeId>) -> Vec<NodeId>;            // 可并行者即扇出
}
```

- **调度确定性是判负与重放的前提**：有序集合＋按 id 破平，故交付顺序不同也排出同一序。环在构造点拒并点名——一个存在的 Workshop 是一个跑得完的 Workshop。
- **契约即 JOB.md**：被派入节点的 Agent 的任务权威就是这份契约本身，机制在 prefix 零常驻。
- **四个字段不许空**（goal／owner／done_check／stop）：空的停止条件是一个不会停的 Run。
- **图的权威是 `Roadmap.md`**，本模块不为节点图另设存储；从路线图行生成契约的那一步未建（四列表不携 `depends_on`）。
- **生产者是 8-4b `workshop_tool`（P1.05）**：在此之前 `NodeContract` 与 `Workshop` 除自身文件外零调用者。

### 8-4b collab::workshop_tool（P1.05；形状 4 适配器）

```rust
pub struct WorkshopDesk { /* who、laid_out: Option<Workshop>、joined: FanIn —— 私有 */ }
impl WorkshopDesk {
    pub fn new(who: String, joined: FanIn) -> WorkshopDesk;
    pub fn lay_out(&mut self, contracts: Vec<NodeContract>, delegates: &mut DelegateDesk)
        -> Result<Vec<NodeId>, AxError>;         // 按 schedule 序逐个走派生台
    pub fn question(&self) -> Result<PrivateQuestion, AxError>;
    pub fn judge(&self, answer: &str) -> Result<Joined, AxError>;
    pub fn accept(&mut self, artifact: Artifact);
}
pub struct WorkshopTool { /* 模型那一面：op ∈ {lay_out, question, judge} */ }
```

- **workshop 是派生的扇出，不是第二条派生通路**：每个节点都过 `DelegateDesk::ask`，故一层深与人的准入两道门是同一段代码。工具的 `Effect` 也是 `Spawn`——摆一张图与派一个人，对人来说是同一个问题。
- **节点的 `JOB.md` 就是契约本身**（`NodeContract::job_text`），不写摘要：摘要即第二个权威。
- **节点 id 就是它的房间地址**，与 `Handback::node()` 同一取法；于是一个节点的身份只有一处。
- **图先自证可跑，再交出去**：`Workshop::new` 在构造点拒重名／悬空依赖／环，故「半张图已派出去、剩下的没人起」这种状态拼不出来。
- **一个 Run 一张图**：第二次 `lay_out` 即拒，因为一个 session 里两张图是「这次在造什么」的两个答案。
- **join 属房间而不属 Run**：子在父冻结之后才开，故 `FanIn` 由装配层按房间保存（`RunWorker.joins`），并与 inbox 折自同一批 `signal_enqueued` 行。`judge` 的围栏一字未改：答不出 digest 前八位即拒，且拒词恒不回显答案。

### 8-5 collab::fanin（P2.06；形状 2 值类型）

```rust
pub struct Claim { /* node、at、digest、by —— 私有 */ }
impl Claim { pub fn verified(self, done_check_passed: bool, verifier: &str) -> Result<Artifact, AxError>; }
pub struct Artifact { /* 只能由 Claim::verified 造出 */ }
impl Artifact { pub fn by(&self) -> &str; }   // 生产者；父要知道活是谁干的
pub struct FanIn { /* BTreeMap<NodeId, Artifact> —— 私有 */ }
impl FanIn {
    pub fn accept(&mut self, artifact: Artifact);
    pub fn question(&self) -> Result<PrivateQuestion, AxError>;
    pub fn decide(&self, answer: &str) -> Result<Joined, AxError>;
}
```

- **只收已验证 Artifact**：未验证的产出是 Claim；`Artifact` 无公开构造子，故「Claim 进汇合」在类型层拼不出来。
- **实现者不自测**（判负线之一）：`verified` 的 verifier 等于生产者即拒。
- **private-info question 是围栏不是证明**：答案由 artifact 内容派生，只有打开过才答得出；能断言的只是「一眼未看就判」被拒。**拒词恒不回显正确答案**——回显即教会那条捷径。

### 8-6 collab::pr（P2.07；形状 5 typestate）

```rust
pub struct Pr<S> { /* node、implementer、branch —— 私有 */ }
pub struct Open;  pub struct Verified { /* by */ }  pub struct Merged { /* by、commit */ }
impl Pr<Open> {
    pub fn open(node: NodeId, implementer: String, branch: String) -> Result<Pr<Open>, AxError>;
    pub fn verified(self, artifact: &Artifact) -> Result<Pr<Verified>, AxError>;
    pub fn opened_payload(&self) -> Result<Payload, AxError>;
    pub fn rejected_payload(&self, by: &str, why: &str) -> Result<Payload, AxError>;
}
impl Pr<Verified> { pub fn merged(self, commit: String) -> Pr<Merged>; }
impl Pr<Merged>   { pub fn merged_payload(&self) -> Result<Payload, AxError>; }
```

- **判负线做成类型**：`Pr<Open>` 没有 `merged`，`Artifact` 没有公开构造子；两条都由 `tests/ui/` 的编译失败反例钉住，不靠评审记得。
- **不重判验证**：`Artifact` 已携「非生产者跑过 done_check」这个事实；本模块只补「这份产出是不是这个节点的」与「验证者不是实现者」这道兜底（近乎不可达，保留是因为「近乎」正在替一场没人做的评审干活）。
- **物理 merge 归 `memory::worktree`**：本 crate 决定，那个 crate 搬文件。merge 只走 fast-forward——trunk 动过即退回重做，与 HeldDraft 同一姿态。

### 8-7 collab::arbiter（P2.08；形状 1 判定）

```rust
pub enum Level { Serialize { after: GoalId }, Arbitrate { with: GoalId }, Owner { with: GoalId, because: Escalation } }
pub enum Escalation { GateRefused, Intent, ArbitrationExhausted }
pub struct Circumstance { pub gate_refused: bool, pub touches_intent: bool, pub arbitration_tried: bool }
pub fn arbitrate(registered: &[GoalEntry], candidate: &GoalEntry, circumstance: Circumstance) -> Option<Level>;
pub fn conflict_payload(candidate: &GoalEntry, level: &Level) -> Result<Payload, AxError>;
```

- **检测进 kernel，仲裁不进**：`kernel::goal::detect_conflict` 只答「撞没撞」；本模块答「谁来裁」。
- **判序固定**（门拒 → 意图 → 仲裁已试 → 机械 → 读）：同一对目标恒落同一级，重放才可比。
- **机械可判的只有一种形状**：双方都claim路径，且常设性一高一低——「常设的先走」不需要任何判断。其余（两个常设、外部资源同名）都要读目标陈述，那是模型的活。
- **机器恒不推翻门**：`gate_refused` 排在最前，压过本可机械串行化的情形；`Circumstance` 三项都是调用方已知而目标条目里看不出来的事实。

### 8-8b collab::delegate_tool（P1.01；形状 4 适配器）

```rust
pub struct Delegated { pub room: Address, pub task: String, pub goal: String, pub kind: DelegateKind }
pub struct DelegateDesk { /* depth: Depth、building: Address、asked —— 私有 */ }
impl DelegateDesk {
    pub fn new(depth: Depth, building: Address) -> DelegateDesk;
    pub fn ask(&mut self, work: Delegated) -> Result<&Delegated, AxError>;   // 门在这里被叫
    pub fn asked(&self) -> &[Delegated];                                      // status.children 的真值
    pub fn take(&mut self) -> Vec<Delegated>;                                 // 回合落定后装配层取走
}
pub struct DelegateTool { /* 模型的那一面：{room, task, goal, kind?} */ }
```

- **`kernel::gate::spawn` 自 S2 建好，到本卡才有第一个生产调用者**。「一层深」不是本模块的判定，本模块只是把问题递给它；拒绝文字也是门自己的三段式，不在这里重写。
- **深度是被携入的，不是被推算的**：`DelegateDesk::new` 收 `Depth`。一个自己推算深度的 Run，错一次就是一个孙代理。
- **一次请求不是一个 Run**。工具答的是「在哪个房间开」，不是结果：在工具调用里驱一个 Run，等于在另一个 Run 的 tool bench 里驱 Run。装配层在父回合落定后取走并派活，子 Run 自己的 `run_started` 携着那个房间。
- **不新增 EventKind**：父的 `tool_called{name:"delegate"}` 与子的 `run_started{addr}` 已经把这件事记了两遍，再加一个事件种类就是第三遍。
- **代理不出楼**：房间必须 `is_within` 父的楼，否则 `E_CROSS_BUILDING_DENIED`，并告知跨楼的正路是 `signal`。
- **回程归 `handback`**（见 8-8c）：本模块只管去程。

### 8-8c collab::handback（P1.02；形状 1 判定）

```rust
pub enum Handback {
    Finished(Artifact),                              // 子自己的 done check 过了，且由非生产者说过
    Stopped { claim: Claim, because: String },       // 停了，或验不过；because 携拒词原文
}
impl Handback {
    pub fn of(claim: Claim, done_check_passed: bool, verifier: &str) -> Handback;
    pub fn by(&self) -> &str;                        // 生产者
    pub fn node(&self) -> &NodeId;                   // 即子房间的地址
    pub fn signal(&self, id: SignalId, to: Address, at: TimeMs) -> Result<Signal, AxError>;
}
```

- **父拿得到子的结果，而那不是下一回合**。子 Run 在父 Run 冻结之后才开（`bin::assembly` 是唯一能造 Run 的地方，而它在驱完父才拿得回控制权），所以「父的下一回合」实际上是**父房间的下一个 Run**。跨 Run 递事实的门已经存在，就是房间的 `Inbox`；再造一扇就是两个权威。故回程走 `Signal`，`status.signals_pending` 自动报数，`signal` 工具自动取得。
- **城市做验证者，不是子自己**：`Claim::verified` 拒绝生产者自验，而 `Completion::Done(Evidence)` 是城市观察到的事实、不是子声明的事实。两者合起来才使 `Artifact` 在这条路上造得出来。
- **一个拒不是一个错误**：验不过也要告诉父，否则父只能靠超时判断。`of` 把 `verified` 的 `Err` 收成 `Unverified` 而不往上抛，是因为在这条路上它是一个**结果**而不是一个故障。
- **不新增 EventKind**：回程落在 `signal_enqueued` 里，与一切其他住房间信号同一形状。
- **为什么不叫 `Verified`**：本 crate 已有 `pr::Verified`（PR 的一个相）。两个同名项一出现，rustc 就不再把路径剪短，`tests/ui/merge_without_verification.stderr` 的预期输出当场变红——**编译器拿一个反例把「一个概念一个名字」执行了一次**。

### 8-8 collab::signal_tool（P3.01；形状 4 适配器）

```rust
pub enum SignalEffect { Enqueued(Signal), Consumed { signal: Signal, by: String } }
pub struct SignalDesk { /* run、room、who、reach、inbox、effects、minted —— 私有 */ }
impl SignalDesk {
    pub fn new(run: RunId, room: Address, who: String, reach: Address, inbox: Inbox) -> SignalDesk;
    pub fn pending(&self) -> u32;                     // 借出前读，status.signals_pending 的真值
    pub fn take_effects(&mut self) -> Vec<SignalEffect>;
    pub fn take_steer(&mut self) -> Option<Steer>;    // P3.07：一件插队信，已属名为 `@发件人地址`
    pub fn take_inbox(&mut self) -> Inbox;            // 归还借出的 Inbox
}
pub struct SignalTool { /* meta、desk: Rc<RefCell<SignalDesk>> —— 私有 */ }
impl SignalTool { pub fn new(desk: Rc<RefCell<SignalDesk>>) -> Result<SignalTool, AxError>; }
impl Tool for SignalTool { /* 两个 action：send｜pull */ }
```

- **Inbox 是借出的，不是拷贝的**：工具在 bench 里被 `Box<dyn Tool>` 包起来，工人再也摸不到它，所以共享句柄走 `Rc<RefCell<..>>`——与 `assembly` 里那个接待批项的 `raised` 同一个手法。整个 Run 期间该房间的 Inbox **恰存一份**，住在 desk 里；驱动返回后无论成败都归还。理由与 `interrupts` 那行注释同字：“a source that stayed behind would be a second one”——两份队列就是两个权威，而漂开的总是没人看的那个。
- **`send` 只入队不投递**：工具只把 Signal 放进 `effects`，真正 `deliver` 到收件房间发生在驱动返回之后、且恒在 `signal_enqueued` 落账之后。因为投影只允许因一条已追加的事件而改变（同 `RunWorker::record`：“the book states what the history says, never what the process hoped to write”）。
- **发件范围由 `reach` 定界**：`reach` 是发件人所属楼的地址，由装配层经 `city::Building::of` 算好传入——**「一个地址归哪栋楼管」的权威在 city，collab 只执行交给它的边界**。越楼发件恒拒，报 `E_CROSS_BUILDING_DENIED` 且三段完整。`ToolMeta.effect` 是静态的（申报为 `Write { domain: room }`），所以逐件目标判定必须在工具内——工具拥有自己的策略。
- **id 不采时钟不取随机**：`{run}-s{n}`，`n` 是 desk 自己的计数器。重放同一段历史得到同一批 id，去重才有意义（确定性第七条）。
- **`take_steer` 取走一件就当场记 `Consumed`（P3.07）**：一件落进窗口的插队信就是已读，不论模型拿它做了什么——否则同一句话会在下一个安全点再落一次，而发件人从历史里看不出它到没到。它与 `pull` 共用同一张队列与同一条 `signal_consumed` 形状，故两扇门没有第二份已读账。
- **`pull` 的剩余量写在结果里**：`status.signals_pending` 是派活那一刻的事实（StatusTool 持的是快照），所以 `pull` 结果里带 `remaining`——一个数字比一套让 status 活起来的机制便宜得多，而且它就在模型正在读的那句话里。
- **投递失败不静默**：`deliver` 返回 `Admission::Shed` 时，入账的是事实而非成功；削峰判定住 `kernel::backpressure`，本模块不自建第二套限流。

### 8-9 collab::goal_tool（P3.01；形状 4 适配器）

```rust
pub enum GoalEffect {
    Registered(GoalEntry),
    Conflicted { entry: GoalEntry, with: GoalId, level: Option<Level> },
}
pub struct GoalDesk { /* run、owner、registered: Vec<GoalEntry>、effects、minted —— 私有 */ }
impl GoalDesk {
    pub fn new(run: RunId, owner: String, registered: Vec<GoalEntry>) -> GoalDesk;
    pub fn take_effects(&mut self) -> Vec<GoalEffect>;
}
pub struct GoalTool { /* meta、desk: Rc<RefCell<GoalDesk>> —— 私有 */ }
impl GoalTool { pub fn new(desk: Rc<RefCell<GoalDesk>>) -> Result<GoalTool, AxError>; }
```

- **三层各守其职，一层不多**：`kernel::goal::detect_conflict` 答撞没撞（纯判定）→ `collab::arbitrate` 答谁来裁（三级）→ 本模块只把两者接成一件工具。它恒不自己判冲突，也恒不自己定级。
- **撞了就不登记**：冲突返回 `E_GOAL_CONFLICT` 三段式拒，第三段是仲裁给的那一级的可执行说法（串行等完某一件｜跟某人商量｜请 owner 裁）。一个只说「不行」的拒绝会让模型换个说法再试一次。
- **`Circumstance` 三项里工具只知道一项**：`gate_refused` 与 `touches_intent` 是调用方才知道的事，机器调用方一律传 `false`；`arbitration_tried` 同理。结果是工具只走得到**机械可判**与**交人读**两条路——这是诚实的：意图判断本来就不归一个参数。
- **同一 Run 内的第二次登记看得见第一次**：desk 把刚登记的条目接在 `registered` 尾上，否则一个 Run 能把同一个资源登记两次而不撞。
- **登记后才入账**：同 8-8，工具只产 effect；`goal_registered` 与 `goal_conflict` 两种事件由工人在驱动返回后写，工人的目标表随之前推。**不写第三种 `arbitration_verdict`**：`conflict_payload` 已携着那一级，再写一条就是同一件事的第二个权威；该事件留给真正跑过一场仲裁的 Run（P3.07 三 Mode）。
- **两个 effect 枚举都是穷尽的**（无 `#[non_exhaustive]`，与本库跨 crate 枚举的常规相反）：每个变体都是工人必须写下的一条账，所以新增一个得是**写账那一端的编译错误**，而不是一条直到某个 Signal 惄悄没入账才有人发现的运行期分支——理由同 `AxCode::carrier()` 的穷尽 match。

### 8-10 collab::pr_tool（P3.02；形状 4 适配器）

```rust
pub struct OpenRequest { pub node: NodeId, pub implementer: String, pub branch: String, pub commit: String }
impl OpenRequest {
    pub fn payload(&self) -> Result<Payload, AxError>;              // pr_opened
    pub fn from_payload(data: &Payload) -> Result<OpenRequest, AxError>;
}
pub enum PrEffect {
    Opened { branch: String },
    Merged { request: OpenRequest, by: String },
    Rejected { request: OpenRequest, by: String, why: String },
}
pub struct PrDesk { /* who、room、branch、node、open、effects —— 私有 */ }
impl PrDesk {
    pub fn new(who: String, room: Address, branch: Option<String>, node: Option<NodeId>,
               open: Vec<OpenRequest>) -> PrDesk;
    pub fn take_effects(&mut self) -> Vec<PrEffect>;
}
pub struct PrTool { /* meta、desk */ }
impl PrTool { pub fn new(room: Address, desk: Rc<RefCell<PrDesk>>) -> Result<PrTool, AxError>; }
// 三个 action：open｜list｜check
```

- **验证与 merge 是一次调用的两个结果**，不是两个 action。一个 `Verified` 而无人 merge 的请求是第三种要人去追的状态；而 merge 不是第二个决定，它就是「验证通过」的含义。拒绝是同一次调用的另一个结果（`passed: false` 携 `why`）。
- **typestate 是走过的不是相信的**：`check` 内部真的造 `Claim` → `verified(true, self.who)` → `Pr::open(..).verified(&artifact)`。于是「实现者不自测」被检查两次：工具先拒（三段式，`E_GATE_DENIED`），类型再拒（`Claim::verified` 的 verifier ≠ 生产者）。
- **被判的是一个 commit 而不是一条分支**：`OpenRequest.commit` 记下开请求那一刻分支站在哪里，Artifact 的 digest 由它派生——**看过一个 commit 的人没有为后一个背书**。
- **没有树的 Run 说得出自己没有**：`open` 在无树时报 `E_TOOL_UNAVAILABLE` 并指向「要审查的楼」，而不是把城里的文件当作自己的产出递出去。
- **谁得到树：楼说了算**（`BUILDING.md` 的 `review: true`，见 `city-SPEC.md`）。默认不开：一个人派一个 Agent 去一个房间干活并盯着看，应当看得到文件变化；为它强制第二个 Agent 是没人要求过的纪律。

### 8-11 collab::triage（P3.07；形状 1 判定）

```rust
pub enum Reflex { Discard, Notify, Light, Full }
pub struct Arrival { pub source: String, pub subject: String, pub tainted: bool }
pub struct Rule { pub matches: String, pub landing: Address, pub reflex: Reflex }
pub struct Landing { pub addr: Address, pub reflex: Reflex, pub because: String }
pub struct Triage { /* rules、fallback —— 私有 */ }
impl Triage {
    pub fn new(rules: Vec<Rule>, fallback: Address) -> Result<Triage, AxError>;  // 空匹配串在构造点拒
    pub fn decide(&self, arrival: &Arrival) -> Landing;                          // 结果恒是一个 Address
}
```

- **结果恒是 Address**，四路只决定「多重的反应体来接」。派活面因此只有一种形状：进来什么，出去的都是一个可派活的地址。
- **规则不是模式语言**：子串、不分大小写、自上而下首中先胜。一张人手写的路由表被读的次数远多于被写，正则会在出事那一刻多一件要 debug 的东西。
- **污染件可以被路由，不可以被开工**：规则写 `Full`，污染件也只到 `Notify`，且降级理由写进 `because`——一个没人能解释的路由决定也是没人能修的。
- **判不出不是错误**，是「一个人读它」：回一个错误只会让调用方去发明一个兼容处理。
- **`because` 恒在场**：它是这层唯一的可观测面，因为四路的差别在别处（谁被派活），不在这个返回值里。

### 8-12 collab::claim_tool（P4.01；V3.19 长出两个动作；形状 4 适配器）

```rust
pub enum ClaimEffect {
    Claimed { id: NodeId, item: String },
    /// 一个被放下的节点。携 `PlanExit` 而不是携一个动词：出口是计划门禁
    /// 造出来的东西，把它的两个臂抄进第二个枚举，就是对「一个节点可以
    /// 怎么离开」的第二份意见。
    PutDown { id: NodeId, item: String, exit: PlanExit },
    Split   { parent: NodeId, children: Vec<String> },
}
impl ClaimEffect {
    pub fn id(&self) -> &NodeId;
    pub fn expected_before(&self) -> RoadmapStatus;
    pub fn kind(&self) -> EventKind;          // 由出口决定，不由调用方决定
    pub fn payload(&self, who: &str) -> Result<Payload, AxError>;
}
pub struct ClaimDesk { /* who、room、roadmap 文本、本次 drive 持有的 Held、effects —— 私有 */ }
impl ClaimDesk {
    pub fn new(who: String, room: Address, roadmap: String) -> ClaimDesk;
    pub fn take_effects(&mut self) -> Vec<ClaimEffect>;
    pub fn roadmap(&self) -> Option<&str>;    // Some 仅当本次 drive 改过；工人据此写盘一次
    pub fn holding(&self) -> Option<&NodeId>;
    pub fn abandon(&mut self) -> Result<(), AxError>;   // 冻结路径花掉仍被持有的 Held
}
pub fn evidence_of(text: &str, id: &NodeId) -> Option<Locator>;
pub fn still_true(text: &str, effect: &ClaimEffect) -> bool;
pub struct ClaimTool { /* meta、Rc<RefCell<ClaimDesk>> —— 私有 */ }
impl ClaimTool { pub fn new(desk: Rc<RefCell<ClaimDesk>>) -> Result<ClaimTool, AxError>; }
```

六个动作：`list`（就绪的、在做的、卡住的）、`claim`（认领一个节点）、`finish`（携证据结项）、`block`（携原因报卡住）、`release`（携原因交回）、`split`（拆成子节点）。

- **`Roadmap.md` 是唯一权威，不另立认领登记表**。第二份登记表就是第二个「这一个节点归谁」的答案，而漂移的恒是没人读的那一份。文件本身既被人读、被 `PlanTree::progress` 数、又被这个工具改——一处事实，三个读者。
- **六个动作长在同一条 catalog 行上，不新开工具**（V3.19 的正面理由）。模型每一轮读的**行数**是成本，一行背后的**动词数**不是。
- **收口是字节数，量出来的**：四个动作的 `plan` 条目是 **548 B**（disclosure ＋ schema 的紧凑 JSON），六个动作是 **547 B**，一条断言钉住它不超过 548。省下的字节来自把 Locator 文法从 schema 移进拒词——**一句重复了拒词内容的说明，是每一轮都在付、只读一次的字节**。schema 里没有的东西，模型第一次写错时会从三段式拒词里拿到。
- **状态迁移由 `kernel::PlanTree` 从计划自身判，不由调用者声明**：`claim` 只从就绪集里取（叶子、无人认领、依赖全绿），`finish`／`block`／`release` 只能作用于**本次 drive 认领的那个节点**。拒词报出此刻的状态并指向一个真能拿的节点——「不行」会教模型改写参数再试，「2.3 在做，2.4 就绪」不会。
- **一次 drive 只持有一个节点**，理由与旧版同：一个 Run 同时占两个节点，两个节点的进度都读不出来。
- **计划门禁就是那个 `Held` 值**（V3.21）：它由 `PlanTree::claim` 铸出，只能花在 `finish`（绿）或 `stop`（红／交回）上。**没有第三个出口**——一个只是结束了的 run 由 `abandon` 把它花在 `FrozeWithoutEvidence` 上，于是「认领了却没交代」这一态在冻结之后不可达。这正是 `blockage` 里红色的来处。
- **`split` 之后本次 drive 不再持有那根枝**：它拿到的那件活现在是几片，它接下来该拿其中一片。写盘前先把新文本重新解析并 `PlanTree::build` 一次，**拆不出合法树就一个字节都不写**。
- **`block` 与 `release` 都必须带一句原因**，且原因**随记录走而不是随表格走**：表格只有位置说「Blocked」，一句话该住在 `roadmap_blocked` 的载荷里，在表里再放一份就是同一句话的第二个权威。
- **哪一种记录由出口决定**（`ClaimEffect::kind`）：绿→`roadmap_finished`，红→`roadmap_blocked`，交回→`roadmap_released`，拆→`roadmap_split`。工人不再自己 match 一遍，于是「停下来意味着什么」只有一个答案。
- **效果穷尽**（同 `SignalEffect`／`GoalEffect`／`PrEffect`，故意不加 `#[non_exhaustive]`）：每个变体都是工人必须写下的一条账，新增一个变体应当是写入处的编译错误。
- **并发口径（诚实边界）**：工人写盘前重读文件，**每个节点只核第一条效果**——一个先认领再结项的 run 两条效果都是它自己按派活时的文件顺序产出的，拿第二条去问磁盘，等于问「我自己刚才那条落盘了没有」，而它没有：desk 是最后整份写一次。行若已不是预期状态则整组丢弃并留一条诊断，而不是覆盖。

## 8.5 两个设计

（两个实质不同的接口方案，按杠杆率与缝的位置比较；落选方案就地留痕。）

**P3.01 第一对（工具怎么拿到活的跨 Run 状态）**：开一个 `peek` 让工具拿快照（落选）vs 把 Inbox 本体借给工具（选中）。快照方案要在 `memory::EventQueue` 与 `collab::Inbox` 两处各长一个新读面，并且它造出两份同时存在的队列——于是「谁才是顺序的权威」多了一个答案。借出方案零新 API，且与 bin 已有的 `interrupts` 借还同形。代价：借出期间工人手上没有该房间的队列，故归还必须在成败两条路上都发生（一条断言盯这件事）。

**P3.02 一对（谁来定一个 Run 写在哪）**：每个 dispatch 都领一棵树（落选）vs 楼的规则说了算（选中）。均一方案读起来干净，但它让每一次普通派活都付一次全量检出的代价，并且把「我派一个 Agent 改一行字」变成「还得再派一个来看一眼」。楼级开关把选择交回给人，且它与 `confidential` 同形同位——一个已有的权威多一行，而不是新开一个。

**P3.01 第二对（三件工具还是一件多臂工具）**：一件 `collab` 工具带 action 枚举（落选）vs 每个机制一件（选中）。合并方案省两份 schema 的常驻字节（工具表坐在缓存前缀里，这是真成本），但它把三件变更理由不同的东西绑成一个接口，而且 disclosure 只能写成一句拉长的话——模型按名字选工具，一个叫 `collab` 的工具不告诉它任何事。三件各自一句话说得清，多出的常驻字节是两份空 schema 的量级。

## 9 工作流程

## 10 实现逻辑

## 11 边界枚举

## 12 错误处理

（逐码回答「能否让它不可能发生」——设计规则十。）

## 13 依赖选型

拓扑硬约束：`kernel` 与 `memory`（ARCHITECTURE.md §2）。新外部依赖随卡论证，无论证即不引。

## 14 硬编码声明

## 15 影响面

## 16 测试与约束

## 17 模型体验

（入窗什么｜token 代价｜对 prefix 缓存的影响；无贡献则写「零字节，因为……」。）

## 18 文档同步
