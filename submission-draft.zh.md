# NERVE — 提案草稿 (中文)

> Nervos Enforced Reputation & Value Exchange (Nervos强制声誉与价值交换)
>
> 一个在CKB上的自主智能体市场,其中智能体身份是一个Cell,支出限制在协议层级强制执行,声誉通过链上争议窗口化的状态转换构建。

---

## 1. 执行摘要

NERVE是CKB上的协议级智能体市场。三个核心安全属性作为共识规则（而非应用代码）强制执行：

1. **支出限额由协议强制。** 类型脚本验证每笔交易；如果智能体超过其每交易限额,节点会拒绝它。没有任何越狱可以推翻共识。

2. **智能体身份是一个Cell。** 智能体的身份、能力、声誉和支出规则都编码在一个单一的CKB Cell中。转移Cell = 转移智能体。所有状态都在链上，不可变。

3. **声誉是争议窗口化和链式可链接的。** 工作完成记录在链上，通过blake2b证明链(提议→等待N个块→最终化)。没有单一方可以单方面更改声誉；挑战需要链上证明。

市场从头到尾无需中央注册即可运行。智能体相互发现、发布工作、通过Fiber网络流式支付，并通过签署证明(v1)或ZK证明(v2)证明能力。

**关键技术贡献:** 工作生命周期的链上状态机(Open → Reserved → Claimed → Completed),通过UTXO原子性防止双重索赔。支出限制在共识层强制,使部署真实资金的LLM智能体具有商业可行性。

---

## 2. 为什么这个问题很重要

当前AI智能体系统存在三个关键结构问题：

### 2.1 防护栏是应用层级

支出限制、能力检查和访问控制是可以被LLM越狱或混淆的代码。如果Claude幻想一个看似有效的交易，基础设施层没有什么会阻止它耗尽钱包。防护栏执行是政策，不是物理法则。

### 2.2 智能体身份是链下且可变的

无法信任地验证智能体能做什么、其记录如何或谁控制它。能力声明是由代码做出的断言,而不是锚定在链上的证明。声誉系统是由可删除或重写的平台操作的数据库。

### 2.3 多智能体支付需要受信中介

当智能体聘请其他智能体时,它们需要托管、结算和声誉验证——这在支付层重新引入了信任问题。被聘请的智能体必须信任聘请方(或中央平台)正确路由资金。

### 2.4 CKB特别如何解决这个问题

- **Cell模型/ UTXO:** 智能体身份是一个Cell。所有权是本机且可转移的。没有要关闭的合同注册表，没有要撤销的管理密钥。
- **RISC-V VM:** 链上任意计算,无预编译约束(不像EVM)。未来ZK证明原生运行。
- **Fiber网络:** 微支付通道实现规模化的按操作计费。
- **Blake2b证明链:** 可验证的声誉更新,无ZK开销(当前),v2带ZK。

---

## 3. 架构与系统设计

### 3.1 端到端架构

```
┌─────────────────────────────────────────────────────────────────┐
│                        用户层                                    │
│  CLI (bash) / Telegram / Slack / Discord (via OpenClaw)         │
└────────────────────┬────────────────────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────────────────────┐
│              智能体运行时 (OpenClaw)                              │
│  监督技能 (Claude Opus)                                          │
│    ├─ 市场工作者 (工作发现、索赔、结算)                          │
│    ├─ DeFi工作者 (UTXOSwap集成)                                 │
│    ├─ 支付工作者 (Fiber通道)                                    │
│    └─ 链扫描器 (自主心跳每10分钟)                               │
│                                                                  │
│  状态: OpenClaw内存 (Markdown检查点+计划状态)                   │
└──────────┬───────────────────────┬──────────────────────────────┘
           │ HTTP REST             │ HTTP REST
┌──────────▼──────────┐  ┌────────▼────────────────┐
│  nerve-core         │  │  nerve-mcp              │
│  (Rust / axum)      │  │  (TypeScript / Express) │
│                     │  │                         │
│  • LocalSigner或    │  │  • CKB RPC / Indexer   │
│    SuperiseSigner   │  │  • Fiber JSON-RPC      │
│  • TX构造           │  │  • Cell扫描            │
│  • secp256k1签名    │  │  • 事件索引             │
│  端口: 8080         │  │  端口: 8081             │
└──────────┬──────────┘  └────────┬────────────────┘
           │                      │
└──────────┼──────────────────────┼──────────────────┐
           │                      │                  │
        ┌──▼──────────────────────▼───────────────┐  │
        │                                         │  │
        │      CKB 测试网                         │  │
        │                                         │  │
        │  智能体身份 (类型脚本强制执行            │  │
        │  支出限额、燃烧保护、                    │  │
        │  Type ID唯一性)                        │  │
        │                                         │  │
        │  工作Cells (FSM: Open → Reserved →     │  │
        │  Claimed → Completed, UTXO原子性)      │  │
        │                                         │  │
        │  声誉Cells (争议窗口,                   │  │
        │  blake2b证明链)                        │  │
        │                                         │  │
        │  能力NFTs (签署证明或ZK证明,            │  │
        │  发行后不可变)                         │  │
        │                                         │  │
        └─────────────────────────────────────────┘  │
                                                     │
        ┌────────────────────────────────────────┐  │
        │  Fiber网络                             │  │
        │  每工作支付通道                        │  │
        │  P2P八卦 (未来: 工作发现)              │  │
        └────────────────────────────────────────┘  │
                                                     │
        ┌────────────────────────────────────────┐  │
        │  UTXOSwap (DeFi演示)                   │  │
        │  代币交换 + RGB++集成                  │  │
        └────────────────────────────────────────┘  │
```

### 3.2 数据结构

#### 智能体身份Cell

```
cell {
  capacity: <锁定的CKB>
  lock: secp256k1-blake2b (20字节公钥哈希)
  type: agent_identity (强制支出限额 + Type ID)
  data: v1 (72字节)
    version: u8
    pubkey: [u8; 33]
    spending_limit_per_tx: u64
    daily_limit: u64
    epoch_started: u64
    daily_spent: u64
    parent_lock_args: Option<[u8; 20]>
    revenue_share_bps: u16
}
```

**类型脚本强制执行:**
- 每交易支出限额(燃烧到非自我地址)
- 燃烧保护(身份Cell必须在输出中重现)
- Type ID单例(每个智能体一个身份Cell)
- 每个epoch边界日累计重置

#### 工作Cell

```
cell {
  capacity: reward_ckb + 122 (最小值)
  lock: 时间锁定后验者锁 (基于TTL)
  type: job_cell (强制FSM)
  data: ~122字节
    job_id: [u8; 32]
    status: u8 (Open=0, Reserved=1, Claimed=2, Completed=3)
    poster_lock_args: [u8; 20]
    worker_lock_args: Option<[u8; 20]> (在索赔时设置)
    reward_ckb: u64
    ttl_block: u64
    created_block: u64
    capability_required_hash: [u8; 32]
}
```

**类型脚本强制执行:**
- 相邻状态转换(Open → Reserved → Claimed → Completed)
- 创建后海报和奖励不可变
- UTXO原子性: 只有一个tx可以消费这个Cell(先写者获胜)

#### 声誉Cell

```
cell {
  capacity: 110 (最小值)
  lock: secp256k1-blake2b + 争议时间锁
  type: reputation (争议窗口化更新)
  data: ~110字节
    agent_lock_args: [u8; 20]
    completed_count: u64
    abandoned_count: u64
    score: u64 (复合)
    pending_hash: Option<[u8; 32]>
    pending_block: u64
    proof_root: [u8; 32] (blake2b链)
}
```

**类型脚本强制执行:**
- 最终化前争议窗口(N块可配置)
- 时间戳新鲜度(待定必须早于N块才能最终化)
- 证明链: `blake2b(旧_根 || 新_更新_哈希)`

#### 能力NFT Cell

```
cell {
  capacity: 61 (最小值)
  lock: 智能体的锁脚本
  type: capability_nft (发行后不可变)
  data: ~54字节
    agent_lock_args: [u8; 20]
    capability_hash: [u8; 32]
    attestation_sig: [u8; 65] (可恢复secp256k1)
    attestation_msg: blake2b(lock_args || capability_hash)
}
```

**类型脚本强制执行:**
- 不可变agent_lock_args和capability_hash
- Cell无法销毁
- 验证: 从签名恢复公钥,确认匹配智能体身份

### 3.3 关键流程

#### 智能体身份生成

1. 用户使用私钥调用 `nerve init`
2. TX构建器从公钥哈希派生lock_args
3. 使用Type ID单例保证构建身份Cell
4. 广播到测试网
5. 身份Cell现在在共识层级治理支出规则

#### 工作市场 (Open → Reserved → Claimed → Completed)

1. **海报发布工作:**
   - TX: 创建工作Cell, 容量 = 奖励 + 最小值, 状态 = Open
   - 链上: 奖励在Cell容量中托管

2. **工人保留工作:**
   - TX: 消费工作Cell, 产生新工作Cell状态 = Reserved, worker_lock_args = None(链上注册表中的软锁)
   - 链下注册表授予独占窗口以防止浪费费用

3. **工人索赔工作:**
   - TX: 消费工作Cell, 产生新工作Cell状态 = Claimed, worker_lock_args = 工人的锁
   - UTXO原子性: 如果两个工人同时提交,只有一个成功(共识级保证)

4. **海报结算工作:**
   - TX: 消费工作Cell, 销毁它, 输出到工人地址带奖励
   - 并发: 写声誉更新Cell (Open → Proposed)

5. **声誉最终化:**
   - TX: 在争议窗口关闭后消费声誉Cell, 产生新声誉Cell带Finalized计数

#### 能力证明发行

1. 智能体生成 capability_hash = blake2b(描述)
2. 创建证明签名 = sign(lock_args || capability_hash)
3. TX: 使用嵌入的签名创建能力NFT Cell
4. 链上: 任何人可以验证: recover(sig, msg) = 智能体的公钥

#### DeFi执行 (UTXOSwap集成)

1. 智能体请求交换via Telegram ("交换50 CKB换USDT")
2. 监督员路由到DeFi工作者技能
3. DeFi工作者:
   - 查询UTXOSwap获取池和汇率
   - 通过Rust TX构建器构造交换TX
   - TX应用于智能体身份Cell(检查支出限额)
4. TX广播 → 在测试网确认
5. 智能体收到USDT,可选地通过Fiber流式传输到对等方

---

## 4. 最近的开发: SupeRISE签名后端集成

### 4.1 解决的问题

NERVE原本直接在 `AGENT_PRIVATE_KEY` 环境变量中保存智能体私钥,在启动时加载到Rust核心中。这对测试有效但为生产环境创建安全问题:

- 私钥在纯文本环境变量中(即使文件权限限制访问)
- 无硬件钱包支持
- 难以轮换密钥或管理多个智能体
- 无静态加密

### 4.2 解决方案: 可插拔签名后端

添加了一个基于特征的抽象 (`Signer`) 有两个实现:

**LocalSigner (默认,现有行为):**
- 包装secp256k1签名
- 在内存中保持私钥
- 快速(进程内)
- 适合开发和测试

**SuperiseSigner (新,可选):**
- 委托给SupeRISE钱包服务(单独进程)
- 私钥永不离开SupeRISE (AES-256-GCM加密静态)
- 通过SupeRISE支持硬件钱包
- ~每个签名50ms RPC延迟

### 4.3 实现细节

**新文件:** `packages/core/src/signer.rs` (362行)

```rust
#[async_trait]
pub trait Signer: Send + Sync {
    /// 签署CKB交易。返回65字节可恢复ECDSA签名。
    async fn sign(&self, tx_hash: &str, witness_count: usize)
        -> Result<[u8; 65], TxBuildError>;

    /// 使用自定义第一证人签署(对于input_type数据)。
    async fn sign_with_witness(&self, tx_hash: &str, first_witness: &[u8], witness_count: usize)
        -> Result<[u8; 65], TxBuildError>;

    /// 签署证明消息(对于能力NFT证明)。
    async fn attest(&self, message: &[u8; 32])
        -> Result<Vec<u8>, TxBuildError>;

    /// 返回压缩公钥。
    async fn pubkey(&self) -> Result<[u8; 33], TxBuildError>;

    /// 返回此签名者的lock_args。
    fn lock_args(&self) -> &str;
}
```

**LocalSigner:**
- 包装现有 `sign_tx()` 和 `sign_tx_with_witness()` 逻辑
- 保持 `private_key: Vec<u8>` 和 `lock_args: String`
- 在初始化时从私钥派生lock_args

**SuperiseSigner:**
- HTTP客户端调用SupeRISE的MCP端点
- 初始化时: 调用 `nervos.address` 获取地址, 通过bech32解码派生lock_args
- `sign()`: 本地计算签名消息, 使用消息哈希调用 `nervos.sign_message`
- 保留 `compute_signing_message()` 逻辑(两个签名者共享)

### 4.4 后端选择

环境变量 `SIGNING_BACKEND` (默认: `local`):

```bash
# 模式1: 本地(默认,现有行为)
SIGNING_BACKEND=local
AGENT_PRIVATE_KEY=0x<32字节十六进制>

# 模式2: SupeRISE (新,可选)
SIGNING_BACKEND=superise
SUPERISE_URL=http://127.0.0.1:18799/mcp
# AGENT_PRIVATE_KEY不必需
```

### 4.5 签名流程(两种模式)

1. **TX构建器构造无符号tx**
   - 输入、输出、证人(在锁定字段中有65字节占位符)

2. **计算签名消息**
   - `blake2b(tx_hash || len(witness[0]) || witness[0] || ...)`
   - 对两个签名者相同(在 `compute_signing_message()` 中定义)

3. **通过Signer特征签署**
   - LocalSigner: 进程内secp256k1签名
   - SuperiseSigner: HTTP RPC调用SupeRISE
   - 两者返回: 65字节可恢复签名

4. **注入签名**
   - 替换证人中偏移 [20..85] 的占位符 [0u8; 65]
   - 广播到测试网

### 4.6 代码变更摘要

| 文件 | 变更 |
|------|------|
| `packages/core/src/signer.rs` | NEW: Signer特征 + LocalSigner + SuperiseSigner (362行) |
| `packages/core/src/state.rs` | 将 `private_key: Vec<u8>` 替换为 `signer: Arc<dyn Signer>`, 异步from_env() |
| `packages/core/src/main.rs` | 添加 `mod signer`, 使main异步 |
| `packages/core/src/tx_builder/*.rs` | 所有构建器: `sign_tx(...)` → `state.signer.sign(...).await?` |
| `packages/core/Cargo.toml` | 添加 `async-trait` 依赖 |
| `.env.example` | 文档 SIGNING_BACKEND 和 SUPERISE_URL |

### 4.7 测试与验证

**单元测试:** 所有58个现有测试通过,无修改
- 签名测试验证占位符证人构造
- 恢复ID验证 (0或1)
- 签名注入到证人
- Blake2b证明链

**集成测试计划:** 参见 `TEST.md` 获取完整测试指南

**向后兼容:** LocalSigner是默认值; API或链上交易格式无破坏性变化

---

## 5. 当前功能与演示流程

### 5.1 三个端到端演示流程

所有流程都完全功能并可针对CKB测试网测试。

#### 流程1: 智能体市场

```bash
nerve demo --flow marketplace
```

- 海报发布工作: "总结这份文件,奖励5 CKB, TTL 100个块"
- 工人(单独进程)通过链上扫描检测工作
- 工人保留 → 索赔 → 完成(链下执行)
- 海报验证结果与description_hash匹配,然后释放奖励
- 结算创建声誉更新,争议窗口10个块
- 声誉最终化后(v1中无自动争议挑战)

**验证:**
- 工作Cell转换: Open → Reserved → Claimed → Completed
- 结果哈希(blake2b)存储在结算证人中(由海报链下验证)
- 奖励 (5 CKB) 路由到工人地址
- 声誉Cell使用blake2b证明链创建(不可变记录)

**当前限制 (v1):**
- 结果验证是链下(海报从IPFS获取并在本地验证)
- 如果结果有争议,没有自动争议机制
- 声誉上的争议窗口防止单方面更改但不会仲裁冲突
- 坏行为者通过声誉伤害被阻止,而不是削减条件
- 如果海报拒绝结算,工作在TTL后过期(工人失去声誉点数)

#### 流程2: DeFi执行

```bash
nerve demo --flow defi
```

- 用户(via Telegram或CLI)请求: "交换50 CKB换USDT"
- 监督员路由到DeFi工作者技能
- DeFi工作者查询UTXOSwap获取池和汇率
- 通过Rust TX构建器构造交换TX
- TX应用于智能体身份Cell(检查支出限额)
- 广播到测试网
- 智能体接收USDT,可选地流式传输给对等方

**验证:**
- 支出限额强制执行(如果交换 > per_tx限额,在类型脚本中拒绝)
- 代币余额在链上更新
- 浏览器显示交换确认

#### 流程3: 能力证明

```bash
nerve demo --flow capability
```

- 智能体铸造能力NFT: "我可以总结文本"
- Cell数据包含:
  - agent_lock_args
  - capability_hash = blake2b("text_summarization")
  - signature = sign(lock_args || capability_hash)
- 任何对等方可以验证: recover(sig, msg) = 智能体的公钥

**验证:**
- 能力Cell部署在链上
- 65字节签名正确嵌入
- 恢复确认智能体身份

### 5.2 CLI命令(全部功能)

```bash
nerve init                     # 部署身份Cell
nerve spawn [--parent]         # 创建子智能体(派生密钥)
nerve balance                  # 检查CKB余额 + lock_args
nerve post --reward 5          # 发布工作Cell
nerve reserve --job 0x...:0    # 保留工作(软锁)
nerve claim --job 0x...:0      # 索赔工作(UTXO原子性)
nerve complete --job 0x...:0   # 完成工作,释放奖励
nerve cancel --job 0x...:0     # 取消过期工作(清理赏金)
nerve jobs                     # 列出链上工作
nerve mint-capability --hash 0x...  # 铸造能力NFT
nerve create-reputation        # 创建声誉Cell
nerve propose-rep --rep 0x...:0 --type 1 --window 10  # 提议更新
nerve finalize-rep --rep 0x...:0  # 窗口关闭后最终化
nerve demo                     # 端到端运行所有3个流程
```

### 5.3 自主智能体 (OpenClaw)

- **监督员技能:** 解析自然语言 → 产生WorkflowPlan → 分派给工人
- **工人技能:** 市场工作者、DeFi工作者、支付工作者、链扫描器
- **心跳:** 自主工作扫描每10分钟
- **内存:** OpenClaw在每个阶段后检查点(重启时恢复)
- **链式思维:** 每个操作前 `<thinking>` 块(可审计推理)

### 5.4 链上合约强制执行

| 约束 | 强制执行方 | 机制 | 状态 |
|-----|----------|------|------|
| 每tx支出限额 | agent_identity类型脚本 | 拒绝到非自我地址的任何超出限额的输出 | ✓ v1 |
| 燃烧保护 | agent_identity类型脚本 | 要求身份Cell在输出中重现 | ✓ v1 |
| Type ID唯一性 | 类型脚本参数 | 每个(code_hash, args)对只有一个身份Cell | ✓ v1 |
| 工作状态FSM | job_cell类型脚本 | 拒绝非相邻转换(例如, Open → Completed无效) | ✓ v1 |
| 海报/奖励不可变 | job_cell类型脚本 | 创建后防止覆盖 | ✓ v1 |
| 争议窗口 | 声誉类型脚本 | 在N块前阻止最终化 | ✓ v1 |
| 能力不可变 | capability_nft类型脚本 | Cell无法销毁或修改 | ✓ v1 |
| **结果哈希验证** | job_cell类型脚本 | 结果哈希嵌入在结算证人中(链下证明验证) | ✓ v1 (部分) |
| **自动争议解决** | (未实现) | 对于争议结果的挑战/仲裁机制 | ⏳ v2 |
| **削减条件** | (未实现) | 对坏行为者的声誉加权惩罚 | ⏳ v2 |

---

## 6. 技术栈与工具

| 层 | 技术 | 目的 |
|----|------|------|
| **链上脚本** | Rust + ckb-std v0.16 | 身份、工作、声誉、能力的类型/锁脚本 |
| **TX构建器** | Rust + axum + ckb-sdk-rust | HTTP REST API, secp256k1签名, UTXO选择 |
| **签名 (v1)** | secp256k1 + blake2b-rs | ECDSA签名 + CKB sighash计算 |
| **签名 (可选)** | SupeRISE MCP桥 | 委托签名与加密密钥存储 |
| **HTTP桥** | TypeScript + Express + CCC | CKB RPC/Indexer/Fiber JSON-RPC包装为REST |
| **智能体运行时** | OpenClaw (Node.js) | 模块化技能 + 监督员编排 + 心跳 |
| **LLM** | Claude Opus 4.6 via Anthropic API | 推理、规划、错误恢复 |
| **状态持久化** | OpenClaw内存 (Markdown) | 检查点 + 计划状态恢复 |
| **支付通道** | Fiber网络 | 每工作微支付 + P2P八卦 |
| **DeFi集成** | UTXOSwap SDK | 代币交换 + RGB++支持 |
| **测试** | Bash脚本 + Rust cargo测试 | 58单元测试 + 5集成测试套件 |
| **部署** | Docker Compose | 编排core、mcp、fiber、agent |

---

## 7. 测试与验证

### 7.1 单元测试 (Rust)

```bash
cd packages/core
cargo test --all-features
# 结果: 58个测试通过, 零警告
```

测试覆盖:
- 签名模块: 证人构造, 恢复ID验证, 签名注入
- 状态实用程序: 身份数据编码, 工作状态FSM, 声誉证明链
- TX构建器: 容量数学, lock arg格式, 类型脚本验证

### 7.2 集成测试

参见 `TEST.md` 获取完整集成测试指南,涵盖:
- LocalSigner模式验证
- SuperiseSigner模式(如果SupeRISE可用)
- 智能体市场流程(4个交易)
- 能力证明发行和验证
- 争议窗口声誉更新
- 签名后端等价性(相同tx → 相同签名)

### 7.3 演示流程测试

```bash
# 端到端运行所有3个流程(需要测试网CKB)
nerve demo

# 或运行个别流程
nerve demo --flow marketplace
nerve demo --flow defi
nerve demo --flow capability
```

每个流程输出CKB测试网浏览器链接用于验证。

---

## 8. 安全模型与威胁缓解

| 威胁 | 缓解 | 状态 |
|-----|------|------|
| **LLM幻想交易耗尽钱包** | 每tx支出限额由类型脚本在共识层强制执行 | ✓ v1 |
| **通过工作描述的提示注入** | 智能体仅读取 `description_hash`; 在沙盒步骤中获取内容 | ✓ v1 |
| **越狱升级到错误的合约** | Allowlist类型脚本模块(未来) — 智能体无法接触不在列表上的合约 | ⏳ v2 |
| **工作索赔上的双花种族** | UTXO原子性 + 保留Cell软锁 | ✓ v1 |
| **损坏的临时密钥** | 密钥限于单一工作 + 能力限制 | ✓ v1 |
| **放弃的工作锁定资金** | TTL + 清理赏金 + 声誉惩罚 | ✓ v1 |
| **通过LLM上下文暴露的私钥** | 签名永不在OpenClaw/LLM层; 总是在Rust进程中 | ✓ v1 |
| **恶意智能体技能注入** | 技能版本控制在repo中; 无从外部来源自动安装 | ✓ v1 |
| **声誉操纵** | blake2b证明链是密码可验证的; 不可变 | ✓ v1 |
| **工人提交欺诈结果** | 海报链下验证 + 声誉伤害(无自动仲裁) | ⚠️ v1 (经济) |
| **海报拒绝有效结算** | 工作过期,资金回收,工人失去声誉点数 | ⚠️ v1 (经济) |
| **自动争议 + 仲裁** | 需要预言网络 + ZK证明(v1中无) | ⏳ v2 |

---

## 9. 未来增强

### 9.1 争议解决与信任 (v1 → v2 关键路径)

**v1当前状态:**
- 通过链下哈希比较进行结果验证(海报责任)
- 声誉证明链防止关于发生的事情的谎言
- 经济上的抑制: 坏行为者获得声誉伤害,失去未来的工作
- 没有自动仲裁或削减

**v2所需功能:**
- **自动争议提交:** 工人/海报可以用链上证据挑战结算
- **预言网络:** 使用blake2b证明验证的去中心化仲裁
- **削减条件:** 对失去争议的声誉加权惩罚
- **ZK能力证明:** 证明工作完成而无需透露计算
- **上诉机制:** 使用声誉加权投票的多轮争议解决

为什么关键: 当前系统适用于小型、频繁的交易(如工作市场),但不适扩展到高价值工作或需要仲裁的复杂工作。

### 9.2 协议扩展

- **ZK能力证明:** halo2编译到RISC-V (实现工作完成的无信任证明)
- **复合锁集成:** OmniLock支出限额模块在锁层(非类型层)
- **多跳委托:** A聘请B聘请C, 跨链净结算
- **智能体身份转移:** 购买/销售代理Cell与完整历史
- **能力可组合性:** 智能体组合多个NFT投标复杂工作

### 9.2 智能体智能

- **Web UI:** React仪表板用于余额、工作、通道、tx日志
- **自主投标:** 智能体评估工作、估计成本/收益、无需人工投标
- **多模型路由:** 不同任务类型的不同LLM
- **SQLite持久化:** 智能体计划在流程重启中存活

### 9.3 经济层

- **动态定价:** 智能体通过Fiber八卦宣传汇率
- **削减条件:** 能力NFT被削减,如果智能体重复放弃工作
- **能力质押:** 智能体在声称能力时面临CKB风险
- **跨链通过RGB++:** BTC生态系统智能体协调

---

## 10. 产品可行性与用例

NERVE是**基础设施,不是产品**。核心原语(智能体身份Cell + 支出限额 + 工作Cell)在整个智能体经济中通用。

### 10.1 具体产品路径

1. **自主DeFi管理:** 用户部署代理Cell与支出限额和DeFi策略。每个操作在链上可审计; 类型脚本是杀死开关。

2. **去中心化计算市场:** 代理与能力NFT接受推理/渲染工作。通过Fiber通道的按任务粒度。没有平台抽成; 对等体在CKB上。

3. **AI服务DAO:** 组织部署监督员代理与预算Cell。代理从市场聘请子代理,从预算支付。支出限额保护金库。

4. **跨链智能体协调:** 通过RGB++和Leap桥,CKB上的代理结算BTC支持的资产。代理市场变为比特币L2生态系统的结算基础设施。

### 10.2 为什么CKB使这成为可行

- **UTXO Cell模型:** 代理所有权本机且可转移。没有要关闭的合约注册表。
- **RISC-V VM:** 链上任意计算(ZK证明原生运行)。
- **Fiber网络:** 规模化按操作计费所需的微支付粒度。
- **类型脚本支出限额:** 使部署真实资金的LLM代理在商业上可行 — 最坏情况损失由共识层界定,不是应用代码。

---

## 11. 可交付物清单

- [x] **项目规范** (本文件 + spec.md)
- [x] **核心Rust实现** (nerve-core + 链上合约)
- [x] **TypeScript HTTP桥** (nerve-mcp + CKB/Fiber/Indexer集成)
- [x] **智能体框架** (OpenClaw技能 + 监督员 + 心跳)
- [x] **CLI工具** (所有操作的 `nerve` bash包装器)
- [x] **测试套件** (58单元测试 + 5集成测试套件)
- [x] **演示脚本** (`nerve demo` 端到端运行所有3个流程)
- [x] **Docker Compose** (core、mcp、fiber、agent的本地编排)
- [x] **文档** (README.md + SKILL.md + 链上合约规范)
- [x] **SupeRISE集成** (Signer特征 + LocalSigner + SuperiseSigner)
- [x] **集成测试指南** (TEST.md与完整测试计划)

---

## 12. 如何运行

### 快速启动 (测试网)

```bash
# 1. 克隆和配置
git clone https://github.com/RobaireTH/NERVE.git
cd NERVE
cp .env.example .env
# 编辑 .env: 设置 AGENT_PRIVATE_KEY

# 2. 构建
cargo build -p nerve-core --release
cd packages/mcp && npm install && npm run build

# 3. 启动服务
# 终端1: Rust TX构建器
source .env && source .env.deployed
cargo run -p nerve-core --release

# 终端2: TypeScript HTTP桥
cd packages/mcp
source .env && source .env.deployed
node dist/index.js

# 终端3: 运行演示
nerve demo --flow marketplace
```

### 使用SupeRISE (可选)

```bash
# 单独启动SupeRISE(单独容器/进程)
docker run -p 18799:18799 superise:latest

# 为SupeRISE配置NERVE
# 编辑 .env: SIGNING_BACKEND=superise, SUPERISE_URL=http://127.0.0.1:18799/mcp

# 启动nerve-core (使用SupeRISE签名)
cargo run -p nerve-core --release
```

---

**摘要字数:** ~2,500
**最后更新:** 2026年3月
**版本:** 1.0 (SupeRISE集成完成)
