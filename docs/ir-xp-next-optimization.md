# IR 热路径与冷编译优化（2026-09-21）

起点为 `935038dd`，包含上一轮性能验证与 CI 修复。本轮修改尚未提交。
本轮之前的正式核心保存为 `build/ir-next-baseline/v86-ir-runtime.wasm`，
SHA-256 为 `212e15e70eeb7fbf79e0f624a495d5f795139c6c1477e0947b0a42a53599ff28`。

## 实现

1. **融合候选提示。** Tier 2 准入仅读取 record 的布尔提示。热边计数跨过门槛、
   发布和回收改变 owner 集合时更新提示；取出候选时仍完整检查融合资格、身份、
   源码和映射。成功/失败的实际融合尝试会清除提示，普通激活不再遍历源集合。
2. **标量 observer 和 IRQ。** 进入 observer 前已经不能继续时，避免复制上下文和
   XMM；返回后先检查便宜的 IRQ 条件，再比较完整上下文、XMM 和所有代码源。
   PIC 共用 `get_irq`，APIC 共用应答前的优先级选择函数。被屏蔽/优先级阻挡的
   请求留在 IRR 中，不再无条件导致退出；回调解除屏蔽后重新检查。PIC cascade
   即使最终无 slave vector，也保留冷路径，因为应答会改变 master 状态。
   查询不依赖尚可能留在 SSA 中的 IF，尤其不能用 backed FLAGS 判断 STI 快路径。
   成功 observer 已完整验证当前 owner，其 admission epoch 证书可被后续普通边
   复用；host、映射、代码等原有 barrier 继续失效该证书。
3. **编译分桶与输入回放。** 每阶段按 tier、普通/共享/融合、单块/多块/未知分桶。
   capture/pipeline 保留选择入口的 PC/tier；lift 前未知的形态不会被误记成单块。
   子桶时间/调用数与原总计守恒。提供只读不可变输入导出和显式离线回放基准。
4. **普通边闭环与多块活跃性。** 热边回到已捕获源码的内部指令时，可以复用现有
   source 闭环，不需要多占一个源名额或先发布独立入口；仍由前端确认指令边界，
   生成代码仍验证实际动态目标，维持四源、边数和图预算。
   多块局部分配先计算每块的 upward-exposed uses/definitions，然后迭代块级集合，
   避免每轮重新扫描所有指令和恢复 StateMap。保留干涉图、类型、恢复状态、MIR
   验证及有界工作预算；工作预算按新算法实际处理的集合计费。

## 相同输入的编译回放

从一次独立 XP 启动导出 **2,540** 个已发布编译产物的不可变源码、地址映射、
多入口和预测边。基准在原生 Rust 中以相同 tier 默认 pass 配置、256 执行预算、
64 REP 预算回放，每版三次。输入解析、文件读取和结果哈希不计入编译时间。
该集合只包含成功发布的输入，不代表 XP 的失败编译、排队和安装成本。

| 三次中位数 | 原块内反复扫描 | 块级 transfer 预计算 | 变化 |
| --- | ---: | ---: | ---: |
| HIR 局部分配 | 1486.88 ms | 1334.96 ms | −10.2% |
| lowering | 3019.65 ms | 2871.89 ms | −4.9% |
| 完整同步编译 | 17150.64 ms | 17079.39 ms | −0.4% |

两版各三次均 2,540 成功、0 失败，代码总大小均为 37,122,456 字节，顺序滚动
校验均为 `b4ed1a18b3c1bd95`：这个输入集合的生成代码没有变化。
局部分配改进可见，但完整编译时间变化很小，不宣称显著的整体编译加速。
这里包含 debug 构建的检查成本，不能将原生回放毫秒数直接当作浏览器 Wasm 时间。

原始文件：`build/ir-next-xp.captures`、`build/ir-next-replay-{before,after}.log`。
原生对照二进制保留在 `build/ir-next-baseline/replay-native` 和
`build/ir-next-baseline/replay-optimized-native`。

## XP 与固定工作量验收

同一会话、同一 JS、镜像和配置，三个后端交替串行各三次；关闭诊断、CPU
sampling、性能记录和输入导出，无本任务的并发构建或测试。镜像仅有内存写覆盖。

| 三轮中位数 | 本轮优化前 IR | 当前 IR | legacy |
| --- | ---: | ---: | ---: |
| 首次 800×600×32 | 44.234 s | **42.104 s** | 22.907 s |
| 到里程碑平均吞吐 | 35.081 mIPS | **36.853 mIPS** | 92.486 mIPS |

当前 IR 启动时间减少 **4.8%**，吞吐增加 **5.1%**。三个配对的优化前/后时间
分别为 45.386/41.190、43.105/42.104、44.234/43.129 秒，三组均加快，但幅度
有波动。时钟轮询令实际退休工作量不同，吞吐比不是严格等工作量的加速比。
不要与此前不同会话的 31/36 秒直接相减判断增益。

| 停止时指标的三轮中位数 | 优化前 | 当前 | 变化 |
| --- | ---: | ---: | ---: |
| 每次激活退休指令 | 15.54 | 18.37 | +18.2% |
| 每百万退休指令的激活 | 51,714 | 43,408 | −16.1% |
| 每百万退休指令的入口完整检查 | 38,699 | 33,149 | −14.3% |
| 每百万退休指令的 observer 检查 | 121 | 6,193 | 增加 |
| 两种完整验证尝试的合计密度 | 38,820 | 39,393 | **+1.5%** |
| IR 覆盖 | 80.00% | 80.55% | +0.55 个百分点 |
| 发布数 | 2,583 | 2,428 | −6.0% |
| 淘汰数 | 1,318 | 1,250 | −5.2% |

每列独立取中位数；验证合计取每轮两类相加后的中位数，不能直接相加表中
两类各自的中位数。检查次数也不等价于检查的字节数或耗时。

固定工作量的 IR/legacy 比：整数 0.962、RAM RMW 0.938、间接区域 1.651、
SSE 寄存器 0.988；几何平均 **1.101，通过既有门槛**。每组客户机退休指令数
相同，最终架构状态相同，IR 覆盖达标。

因此：本轮实现带来可测的有限改善，尚未实现激活长度翻倍、完整验证总密度
下降或与 legacy 持平。当前 XP 吞吐仍为 legacy 的 **39.85%**，启动时间为
legacy 的 **1.838 倍**。`xp_compare` 退出 1 对应这个性能门槛失败；九次 VM
均完成里程碑，不是启动或正确性测试失败。后续主要问题仍是短普通路径的边界
成本和 observer 证书成本，不能因 helper 退出减少就认定问题已解决。

原始结果为 `build/ir-next-xp-summary.json`、`build/ir-next-xp-*-*.jsonl`、
`build/ir-next-fixed.jsonl`。当前正式核心 SHA-256：
`0431173ab68f11fe6770b6581daaccdb6fe851ab3ea0d34eab0f847789f32456`。

## 验证命令

本地验证结果：

- 原生测试 248 通过，2 个显式微基准忽略；production 和 ir-experimental
  的 Wasm `cargo check --release` 均通过 `-D warnings`。
- `ir-entry-tests`、`ir-live-tests`、`ir-cache-tests`、`ir-auto-tests`、
  `ir-fusion-tests`、`ir-diagnostic-tests`、`ir-sti-tests`、`ir-helper-reload-tests`
  全部通过，包括 debug/release 与无测试钩子的正式核心。
- STI/nested shadow 每版 1,026 案例；helper reload 每版 182 案例。
  新增测试覆盖 masked/in-service IRQ、回调解除屏蔽、STI 留存 masked request、
  四源内部目标闭环，以及每个编译阶段分桶的调用数/耗时守恒。
- APIC 原生测试以不同 vector、ISR 和 TPR 比较只读候选与真正应答结果，并检查
  查询不修改 IRR/ISR/TPR。PIC 沿用原应答的同一个只读选择函数。
- staged decode、多入口、无 SIMD 便携构建、helper audit、浏览器主线程和
  Worker 的 backend/SMC/save-restore/错误路径均通过。

日志：`build/ir-next-native-final.log`、`build/ir-next-regression.log`、
`build/ir-next-check-{production,experimental}.log`、`build/ir-next-contracts.log`、
`build/ir-next-helper-audit.log`。这是本地验证，不代表远端 CI 已运行。

```sh
RUSTFLAGS='-D warnings' cargo test
make ir-entry-tests ir-live-tests ir-cache-tests ir-auto-tests ir-fusion-tests \
  ir-diagnostic-tests ir-sti-tests ir-helper-reload-tests
IR_CAPTURE_FILE=build/ir-next-xp.captures IR_BOOT_TARGET=desktop IR_BOOT_MS=180000 \
  node tests/ir/performance/xp_boot.mjs /path/to/windowsxp.img ir
IR_CAPTURE_FILE=build/ir-next-xp.captures \
  cargo test compile_capture_replay -- --ignored --nocapture --test-threads=1
IR_BASELINE_WASM=build/ir-next-baseline/v86-ir-runtime.wasm \
  node tests/ir/performance/xp_compare.mjs /path/to/windowsxp.img build/ir-next-xp
node tests/ir/performance/fixed_work.mjs > build/ir-next-fixed.jsonl
```

回放计时为显式 ignored 基准，不在 CI 中设机器相关的速度门槛。
导出输入、诊断和 CPU sampling 均用于定位问题；最终 XP 验收必须全部关闭，
使用同会话交替串行的三轮配对。显示里程碑仍是首次 800×600×32，不是桌面空闲。
