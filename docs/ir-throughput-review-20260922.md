# IR 全面审查与第二轮吞吐优化（2026-09-22，PR #54）

## 结论与基线

本报告是 [实施计划](v86-ir-implementation-plan.md) 和 [历轮进度](ir-progress.md) 的本轮审查补充。基线固定为 **`3da2deb731143203430f86c69f1c57ed550d71f5`**，即已合并 PR #53 的 `ir` 分支；本 PR 的目标也是 `ir`。

**已实现两项热路径优化并补充边界测试，但没有完成全部 IR feature，也没有证明 Windows XP 超过 legacy。** Core 和 vector 固定工作量性能门槛均失败；实际 XP 尝试在镜像下载阶段遇到 HTTP 404，没有生成 XP 性能结果。因此 PR 保留 Draft，生产默认仍是 legacy，IR-14 不启动。

PR #53 已经实现的向量 RAM 写续执行、16 类基本 SSE FP 寄存器快路径、纯尾段预算批处理和无编译工作时的 scheduler hint，不计为本轮新增。本轮扩展的是**有访存/helper 的有界混合循环预算批处理**，以及**非观察型预算退出后的准入证书复用**。

## IR-00～IR-14 逐项核对

下表保持原计划的职责划分。“已完成”仅指进度文档明确验收的职责，不将基础设施完成等同于所有 ISA 或真实系统通过。

| 工作包 | 已有设计及审查判断 | 本轮处理与仍缺的验收 |
|---|---|---|
| IR-00 | 后端选择、优化级别、pass/诊断开关、基线工具已有实现 | 保存四路配对数据和精确构建标识；真实 XP/应用基线仍缺 |
| IR-01 | Builder 的宽索引、LEB、typed locals/signatures 已完成 | 复用现有机制，不重写；保留代码大小约束 |
| IR-02 | 共享 decoder 和不可变代码快照已进入实际编译路径 | 保持生成一致性；全部前缀、特权、子编码和跨页取指组合尚未逐项验收 |
| IR-03 | HIR/owned-MIR 分离，类型、effect、控制、状态计划已有 verifier | 新增混合循环预算证书；更广泛 MIR 图变换与证明仍不能宣称完成 |
| IR-04 | StateMap、helper outcome、CPU reload、观察与异常所有权已有契约 | 保留原恢复点，扩展 Poll 退出协议；完整 helper/回调边界仍须随新增范围审计 |
| IR-05 | 整数、FLAGS、条件和移位等已有原生 lowering | 减少热点循环重复预算检查；完整模式与前缀矩阵未收口 |
| IR-06 | 普通 RAM 快路、精确 MMU/MMIO、RMW 与代码别名保护已存在 | 混合循环保留每个原始 epoch 检查；未放宽 alias、故障或访存提交规则 |
| IR-07 | 控制流、REP 和复杂系统操作有 native/helper 覆盖 | 不把覆盖等同于低开销实现；完整 OS、设备、特权转换验收仍缺 |
| IR-08 | XMM/整数 SIMD 与部分 FP 已原生化，x87/MMX/其他 FP 仍大量依赖 helper | 本轮摊薄相关循环预算成本；未新增 ISA、通用 FP lowering 或 fast-math 假设 |
| IR-09 | 自有 backend/Tier-1、通用 reducible CFG structurer 已按原边界完成 | 扩展循环发射；不可约 CFG 继续使用自有 dispatcher fallback |
| IR-10 | FLAGS、DCE、GVN、copy、CFG、helper-state 六类基础优化已验收 | 复用现有 pass 开关和差分，不重新计为本轮完成 |
| IR-11 | 有界 SSA/memory LICM、RAM proof/forwarding 的既定证明范围已验收 | 新增受限混合自循环和小 latch 批处理；不是任意多块循环/trace 融合 |
| IR-12 | 缓存、发布、owner、generation、mapping、失效生命周期已验收 | 区分非观察型 Poll 和真正观察边界，保留有效准入区间；进一步降低切换成本仍有空间 |
| IR-13 | 微基准、差分和 host/browser/device smoke 框架已有实现 | 本轮新增测试通过，但两个性能门槛失败；XP、真实应用和冷热端到端验收未完成 |
| IR-14 | 默认 legacy，旧 emitter 未退役 | 保持原状；未降低生产准入条件 |

当前生成目录仍是 **935 个编码记录、3,972 个粗粒度形式、3,728 production Pending、0 experimental Pending**。实验 Pending 为零包括 native/helper/显式基线行为，不代表全部原生化，更不代表完整前缀、模式、异常和生产工作负载验收。本轮没有修改这些覆盖结论。

## 发现与实现

### A. 混合热点循环没有受益于原来的纯尾段预算批处理

原优化对纯计算尾段有效，但 RAM、SSE helper 等常见混合循环仍重复执行预算检查。直接复制完整快慢执行体的第一版实验出现回退，未作为最终方案保留。最终实现先验证整次循环所需的 dispatcher credits；足够时执行 prepaid 热循环，将不足一次完整迭代的原始执行体剥离到热回边之外，减少热循环内部合流负担。

预扣的是 **dispatcher 工作预算，不是 guest 指令退休数**。原始状态物化、退休点、故障和 helper outcome 不变；混合循环保留原位置的代码 epoch 检查。小 bookkeeping latch 的成本纳入证书，走另一条正常边时退还不执行的 latch credits。预算不足时仍沿原逐指令路径恢复，而不是跳过待执行 guest 指令。

证书由 verifier 独立重算。限制包括 262,144 work units、每个 body 最多 512 条 MIR 指令、31 个内部 poll、每个产物最多 4 组批处理，以及最多 8 条指令/8 个 value steps 的小 latch。原 256 KiB 发射大小限制、Tier 与诊断/interrupt-shadow 限制保留。没有推广为任意控制流 speculative execution。

主要文件：`src/rust/ir/mir/budget.rs`、`src/rust/ir/backend/wasm.rs`、`tests/ir/semantics/budget_batch.rs`、`tests/ir/differential/budget_batch.mjs` 和新增 `budget_observers.mjs`。

### B. 纯预算退出也无条件结束准入验证区间

此前已经验证过的热点在预算耗尽后返回 CPU，下一次准入又可能重复做字节/映射验证。本轮把退出原因收敛为 **None / Normal / Observer / Poll**，由编译器生成 `ir_request_poll_exit()`，明确标记纯预算恢复，而不是将它误当成可直接链接的出口。

只有策略开启、没有要求观察的 profiling/recording 状态且退出原因确为 Poll 时，才允许保留现有验证区间。**保留区间不等于无条件通过下一次准入**：generation、mapping、代码 epoch 和 owner 仍必须符合原条件；helper/解释器/host batch/代码写入等边界照常失效。没有新增 unchecked `call_indirect`，没有扩大默认执行预算，也没有删掉 IRQ 或 SMC 检查。

新增静止态 A/B 开关 `ir_cache_set_poll_reuse(0|1)`；非法值或活动帧内切换被拒绝，策略变更自身会使当前区间失效。`ir_cache_stat(39)` 统计省去的重复屏障，`ir_cache_stat(40)` 返回开关状态。测试同时核对实际 fast validation 命中，不能仅凭“省去屏障”计数宣称证书已被复用。

主要文件：`src/rust/ir/runtime/entry.rs`、`runtime/cache.rs`、`helper/imports.rs` 和新增 `tests/ir/differential/poll_reuse.mjs`。

### C. 基线主 CI 被格式检查阻塞

基线存在 rustfmt 和 ESLint 失败，导致后续回归被跳过。修复单列为机械提交，便于与性能代码分开审查。Rust 依仓库格式配置重排；JS 修改包括作用域感知的局部绑定重命名，保留公开 import/export/property 名称，不能简单描述为“全部只是空白变化”。Closure 公共别名保留，仅对必要自赋值作局部说明。

生成的 ENCODINGS 表由生成器输出 item 级 `#[rustfmt::skip]`，使精确再生成检查和格式检查相容；没有关闭全局格式或语义检查。已对实际发布的格式化后源码和 CI Wasm/JS 产物重新执行关键回归。

## 正确性验证及范围

| 验证 | 已观察结果 |
|---|---|
| 原生 Rust 全套 | 258 passed、0 failed、2 ignored；`RUSTFLAGS=-D warnings` |
| MIR/Wasm 可执行测试 | 通过，包括真实生成模块执行；不是只校验字节合法性 |
| 精确预算差分 | debug/release 各 39,600 组，每个构建累计核对 517,808 guest steps |
| 观察/故障差分 | debug/release 各 2,200 组：696 个精确故障、320 个 callback cases、16 个 checked epoch exits |
| Poll 策略 A/B | runtime/debug-cache/release-cache 各 108 组；每个构建省去 17,433 次重复屏障，实际 fast validation 命中 17,421 次 |
| 既有 warm-chain | 三种构建各 12 组配对、6,156 次 handoff；包含 SMC、remap、owner 替换、reset、XMM 与 host batch |
| missing-hint | 三种构建均通过 |
| 发布产物复测 | CI 格式化后源码的 ESLint、decoder 再生成、两种 CPU 的预算/观察差分及 runtime 的 Poll/warm-chain/missing-hint 均通过 |

原生全套在本地使用 `CARGO_PROFILE_TEST_OPT_LEVEL=0 CARGO_PROFILE_TEST_DEBUG=0`，上述结果不是该配置的性能测量。外部 decoder oracle 在本地因 `ndisasm` 缺失未执行；不能将其算作通过。标准 CI 安装 nasm 并保留该 oracle。

边界说明：测试覆盖原预算切点、16/32 位、非零 CS、计数回绕、FLAGS、XMM、高 lane 特殊值、故障/MMIO、host batch 之间 raw/notified 改码和诊断降级。对没有 code-epoch 契约的 unfused 离线 fixture，未把回调改写未来 opcode 的行为计入本轮已验证的 SMC 结论；该类改码验证限于具备相应契约的 fused fixture。没有用大量测试通过代替未覆盖边界的证明。

CI 构建/测试快照为 PR 合成提交 `13425ccb896323669f07b32f512af0c2cb6e9ece`，对应 head `9cf7f1e61a0ec3b4821d9fcef6253489503d8a21`。复现产物来自 Actions run `35748436480`。该产物与下节本地计时产物不同；后续只改文档/workflow，不重标为已测最终二进制。

## 最终性能对照：两套门槛均失败

本地 Node 22.16.0，CPU affinity 0，固定 guest 工作量，scale 20，五轮，current IR/current legacy/baseline IR/baseline legacy 四路顺序交替。每个样本先预热到 Tier 2，核对退休数、终态和实际 IR 覆盖。计时范围为 **start-to-observed-halt**，不包含另行记录的 `vm.stop()` 等待；不是单独的生成代码执行耗时。

宿主为共享环境，方差明显；本表是样本中位数，不是具有置信区间的稳定增益估计。旧、新构建的 legacy 对照也有波动，不能把其中一个数字当成硬件恒定能力。必须结合 [全部四路原始 mIPS 样本](measurements/ir-throughput-20260922.json) 阅读，尤其不能外推 XP 平均 mIPS。

| 工作负载 | 旧 IR mIPS | 本轮 IR mIPS | 同轮 legacy mIPS | IR 相对旧 IR | IR / legacy |
|---|---:|---:|---:|---:|---:|
| integer | 880.94 | 944.18 | 579.00 | +7.2% | 1.631 |
| ram_rmw | 389.96 | 426.51 | 588.74 | +9.4% | 0.724 |
| indirect_regions | 422.48 | 373.88 | 66.47 | -11.5% | 5.625 |
| sse_register | 431.68 | 427.97 | 720.40 | -0.9% | 0.594 |
| sse_packed_double | 415.98 | 579.65 | 804.49 | +39.3% | 0.721 |
| sse_scalar_single | 449.21 | 605.02 | 570.61 | +34.7% | 1.060 |
| sse_scalar_double | 520.98 | 664.57 | 533.65 | +27.6% | 1.245 |
| xmm_store | 418.90 | 472.06 | 510.82 | +12.7% | 0.924 |
| xmm_high_store | 351.22 | 461.30 | 626.63 | +31.3% | 0.736 |
| xmm_masked_store | 231.15 | 215.88 | 72.65 | -6.6% | 2.972 |

原门槛保持 **几何均值 ≥ 1 且每一项 IR/legacy ≥ 0.9**。

- Core 几何均值 **1.410，FAIL**：RAM 0.724、packed-single SSE 0.594 未达单项要求。
- Vector 几何均值 **1.115，FAIL**：packed-double SSE 0.721、high-half store 0.736 未达单项要求。

本轮相对旧 IR 的回退也保留：间接区域约 **-11.5%**、masked store **-6.6%**，packed-single SSE 约 **-0.9%**。整数、RAM 和若干标量/packed-double SSE 改善，并不构成整体性能已超过 legacy 的结论。

两套脚本均以状态 1 结束，因为性能 gate 为 FAIL；不是把失败改成通过。最初重复快慢块的方案及仅 loop-peeling 的中间实验日志也保留在交付日志包中。

### 计时与发布产物必须区分

| 产物 | SHA-256 |
|---|---|
| 本地计时 A+B Wasm（机械格式修复前） | `b600b0eb6dda329f8a0ad7e702238e844a3fbf1ae74674df730d0eecb0910501` |
| 本地配对旧 IR Wasm | `f326c81b08397073039d7bca969119a4442be4b66c140f7b98972926f56a2e1e` |
| CI 发布并做关键正确性复测的 runtime Wasm | `0b2561a30e81137e9ed23095400bfd99ee66efc4de2447b909546381bc2600e8` |
| 对应 CI libv86.mjs | `fd97bf6491b15933532f172144d2b72b2bbcea19bda94f5e095cb6ba9bd34367` |

没有宣称不同目录、构建配置和源码阶段的产物位级相同，也没有将发布产物的正确性复测冒充性能重测。

## XP 真实系统尝试：镜像下载 404

按用户站点 `retro-gaming-site/app.js` 的提交 `150f39493c2ad39b78b6d10bfa6a248a61e8f83b` 解析 `R2_URL_1 + game.systemDisk`，尝试其 2 GiB XP 系统盘。Actions run **35748398258** 已成功构建固定基线和本轮 runtime；在 **2026-09-22 15:37:13 UTC** 下载镜像返回 HTTP 404，之后的三轮启动配对被跳过。

这不是 XP 启动成功，也不是 XP 兼容性失败证据：没有取得可运行的 guest 输入。因此本轮 **没有 XP mIPS、桌面启动时间或游戏帧率结果**。历史报告中的 4 GiB 系统盘/首个显示模式结果不能替代本轮，更不能与用户的桌面 avg mIPS 混算。

当前 `ir-xp-review.yml` 改为手动执行，需要获授权的公开 HTTPS 镜像地址、解压后大小和 SHA-256。只保存测量/构建信息，不上传 guest 系统盘。现有 XP harness 的 RAM/VGA 配置仍须在实际验收时与用户使用配置核对，不能仅因盘名接近便认定工作负载一致。

## 复现与回退

```sh
# 常规验证入口；需要项目既有 Rust、Wasm、Node、nasm/ndisasm 工具链
make ir-generated-check ir-budget-batch-tests ir-cache-tests
RUSTFLAGS="-D warnings" cargo test
node tests/ir/wasm/run.mjs

# 同一台机器保存基线 Wasm，连续执行两套固定工作量矩阵
IR_COMPARE_RUNS=5 IR_FIXED_SCALE=20 \
  node tests/ir/performance/fixed_work.mjs build/v86-ir-runtime.wasm /path/to/baseline.wasm
IR_COMPARE_RUNS=5 IR_FIXED_SCALE=20 IR_FIXED_SUITE=vector \
  node tests/ir/performance/fixed_work.mjs build/v86-ir-runtime.wasm /path/to/baseline.wasm
```

Linux 上应给配对命令指定同一允许 CPU；其他平台使用该平台的等价方法，不照抄 CPU 0 假定。不要并行跑编译和性能测试。每次保存 Wasm/JS/镜像哈希、配置、Node/浏览器版本、冷热阶段及完整样本。

预算批处理可用已有 `budget_batch` pass disable bit 17 回退。Poll 复用可以在静止态通过 `ir_cache_set_poll_reuse(0)` 关闭，策略切换触发原失效规则。没有调整默认后端、默认预算、CPUID/TSC 或严格浮点策略来抬高计数。

## 未收口问题与合并判断

从本轮代码及测量能确认两个重复成本，但不能证明它们解释了用户 40 对 100 mIPS 的全部差距。热循环跑分也无法评价冷启动编译、解释回退比例、实际每次 activation 的 guest 指令数，以及频繁系统/设备操作导致的状态物化。

下一轮验收所需证据是：同一可用 XP 镜像/快照及配置、固定应用操作、冷启动和预热分别测量、生成代码/编译/解释器/准入/回调的耗时与计数分解，以及有观察者边界的真实热点重放。在此基础上决定安全区域扩展、降低状态物化/准入开销还是减少编译成本；不能仅提高执行预算或扩大未经证明的 fusion 范围。

**合并判断：本 PR 是已实现并验证边界的优化实验和审查交付，不是已通过性能验收的生产切换。保持 Draft 和 legacy 默认；完整 CI、现有单项性能底线及 XP 端到端验收尚不能标记完成。**
