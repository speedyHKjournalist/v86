# IR 诊断、独立校验与 helper 合并增量（2026-09-20）

本轮补齐公开 dump/verify/stats 配置，并扩展独立 MIR 校验、CLI 状态保留及
含审计 helper 的三/四区域合并。尚不能据此宣称 IR-00～IR-13 全部完成。

## 实现

- `ir_verify=off|debug|every_pass`：必要 lowering/proof/发布检查始终保留；debug
  在断言构建检查 MIR 边界，every_pass 在 release 也检查每个机器优化阶段。
- `ir_dump=off|hir|mir|wasm|all`：16 条有界编译记录，文本各 64 KiB、Wasm 256 KiB，
  UTF-8 安全截断与截断标志。`get_ir_dumps(clear)` 复制读取，Worker 不暴露活指针。
- `ir_stats=off|sampled|debug`：默认关闭，分别对应采样周期 0/128/1；已有动态
  配置 API 继续可用。三项均经过主线程/Worker、URL 保留、初始化校验和查询路径。
- `MirRegion::verify()` 不需要 HIR：检查图所有权、定义与使用/支配、状态表达式、
  typed local 干涉、平行复制、helper 结果及 import/RAM 证书。验证有工作预算。
  新增损坏图拒绝测试，包括越界、缺失定义、locals 冲突和非法状态/边。
- CLI 正常实模式或 ring-0 非 VM86 路径不再写回全部 GPR/算术 FLAGS；其他路径在
  可能触发异常前保留完整状态恢复和失效屏障。
- 三/四源图接受 code-preserving helper 和审计过的寄存器 SSE；STI shadow、
  通用 host/内存 observer 仍保守拒绝，不把此前未定位的 XP 异常宣布修复。

## 验证记录

- 234 项原生测试通过：`build/ir-debug-final-native.log`。
- 新增 192 组三/四源 CLI/SQRTSS 差分；原有 1536 组 fusion、MMIO raw 写入、
  代码物理别名及 #PF 对照通过：`build/ir-debug-helper-fusion.log`。
- 系统模式/权限/异常矩阵通过，并直接检查正常 CLI 不发生完整状态写回：
  `build/ir-debug-helper-system.log`。
- 主线程、真实 Worker、Node 和无 SIMD 核心的公开 dump/verify/stats、初始化拒绝、
  副本隔离和 reset/restore 场景通过；日志为 `build/ir-debug-final-{backend,browser,portable}.log`。
- 初轮固定工作量三轮几何平均 1.071 倍，通过既有门槛，但各单项并非全部超过 legacy。
  `build/ir-debug-fixed-work.jsonl`。
- 初轮 XP 三组配对 **未通过**：IR 33.409 秒、46.577 mIPS；legacy 18.963 秒、
  111.415 mIPS。里程碑仍是首次 800×600×32，不等于桌面空闲。原始结果
  `build/ir-debug-xp-summary.json`。这些数值在下述追加优化之前测得。

## 剩余验收边界

完整的 host/设备观察点选择性同步、STI shadow 和通用 helper 的三/四源继续执行
仍需要各自的状态及失效证明。公开配置或图校验的完成，不能替代这些契约。
IR-13 采用用户确认的 XP 冷启动与固定 CPU 工作量；第一项仍须达到 legacy 的
配对中位数水平，固定工作量沿用既有精确状态/指令数门槛。默认后端保持 legacy。


## 继续消除 lowering 和观察点开销

初轮 XP 未达标后，继续实现：

1. lowering 保存私有 allocator 结果及不可变 HIR 身份凭据，收口时校验凭据，
   不再重新执行同一个 allocator。所有 Draft 修改/换源负例仍须拒绝；独立 MIR
   verifier 另行检查活跃值干涉，不把重复运行同一算法视为独立正确性证明。
2. CpuExit/CpuRep 终端观察点允许省略与入口 CPU backing 完全相同的状态写回。
   修改过的寄存器/FLAGS、EIP、previous IP 和退休计数保持原协议；任何先前存在
   的可继续访存/观察点仍会保守禁止这类入口证明，避免错误复用过期 backing。
3. CPU liveness 改由实际 owned MIR 和已生效的状态/helper 裁剪结果推导，在 Tier 2
   启用时只运行一次。Tier 1 不再预计算两套 HIR 候选 mask；重复状态写回对应的
   无用 CPU 读取也能消除。工作预算耗尽保守保留所有值；独立校验拒绝丢失所需值。
4. owned MIR verifier 另加 effect 链、缺失机器计划/实际定义、RMW ticket 单次消费、
   宽度、权限 guard 和提交状态检查。

236 项原生测试通过（`build/ir-demand-complete-native.log`）。CPU 信息/权限/TSC
回调、标量/字符串 I/O 的已有完整矩阵以“状态裁剪 + 新 liveness”组合重新验证；
`build/ir-terminal-{cpu-info,io}.log`。另有 66,048 CFG 对照、33,024 精确预算出口、
GPR/FLAGS/XMM helper reload 与回调变更对照通过（`build/ir-demand-{cfg,reload,ir10,forwarding}.log`）。
后续性能验收须使用追加优化后的核心，不能拿初轮数字作最终结果。

## 观察点之后的同步证明

owned MIR 另行追踪基本块内的 CPU backing 读取与正常 CpuReload 结果。后续
StatePlan 对相同地址、宽度和 SSA 值的写回可以省略；同一 StateId 的全部使用点
取交集。访存、effect 和 helper 清除旧事实，caller-owned fault delivery 在恢复
之前也清除；native FP 的双分支不会被 helper 分支单独授权。EIP、previous IP
和退休计数保持原阶段。预算耗尽保守关闭这部分优化，独立 verifier 重算证书。

组合差分测试发现并修复了一个 liveness 问题：选择性 SSE adapter 与 native FP
分支直接消费 StatePlan 中的操作数，即使对应的完整状态写回被省略，这些值仍
是必须保留的机器输入。增加独立回归测试，并在 helper reload 的四种编译变体
中启用状态裁剪与 CPU liveness 组合。debug/release 的 182 组 GPR/FLAGS/XMM、
回调变更、CFG 循环、上下文/代码失效和故障对照均通过。

这项证明覆盖基本块内的观察点前后窗口；没有宣称任意 CFG 合流、通用设备 helper
选择性 ABI 或含这些 helper 的任意三/四源图已经全部实现。

## 追加优化后的回归与固定工作量

- `build/ir-sync-final-native.log`：238 项原生测试通过。
- `build/ir-sync-{cfg,io,cpu_info,reload,forwarding,fusion}-validated.log`：
  CFG、精确预算、故障、I/O 回调、SSE reload 与多源合并差分通过；release fusion
  另见 `build/ir-sync-fusion-release.log`。
- `build/ir-sync-{backend,auto,browser,portable}.log`：生产自动升档、主线程、
  真实 Worker 与无 SIMD 核心公开接口通过。
- 已重建 runtime、debug/release 测试核心和无 SIMD runtime。

三轮固定工作量对照保持精确退休指令数、最终 CPU 状态及至少 95% IR 覆盖要求，
无并发构建或测试，原始数据 `build/ir-sync-fixed-work.jsonl`：

| 工作量 | IR 中位 mIPS | Legacy 中位 mIPS | 比值 |
| --- | ---: | ---: | ---: |
| 整数 | 175.914 | 186.706 | 0.942 |
| RAM RMW | 107.913 | 117.695 | 0.917 |
| 间接区域 | 113.282 | 66.846 | 1.695 |
| SSE 寄存器 | 50.770 | 51.714 | 0.982 |

几何平均 1.095 倍，通过既有门槛；不表示每项都超过 legacy，也不能代替 XP 验收。

同一 XP 镜像三组交替配对、关闭诊断、使用 RAM 写覆盖层，结果仍然 **FAIL**：

| 首次 800×600×32 显示模式 | IR 中位数 | Legacy 中位数 |
| --- | ---: | ---: |
| 时间 | 33.717 秒 | 19.251 秒 |
| 退休指令吞吐 | 45.688 mIPS | 110.088 mIPS |

时间比 1.751、吞吐比 0.415；两个既有门槛均未达到。原始结果为
`build/ir-sync-xp-summary.json`。这不是桌面空闲或游戏性能结果。与初轮相比，
不能据此宣称稳定提速；新增功能不等于 IR-13 性能目标已经完成。

## 本轮耗时归因

另行运行 CPU profile + 周期 16 诊断，原始文件：
`build/ir-sync-xp.cpuprofile`、`build/ir-sync-xp-diagnostic.jsonl`；摘要为
`build/ir-sync-xp-{profile,diagnostic}-summary.json`。该轮约 38.1 秒，受采样
扰动，不能与关闭诊断的配对成绩混用。

V8 栈采样分组占比（含 12.4% idle）为：入口/分派 22.6%、解释器 18.9%、
编译器 13.3%、生成代码及其 helper 12.3%、调度 8.8%。编译器全调用计时：
lowering 合计 2.466 秒、最长 47.81 ms；机器优化合计 0.573 秒、最长 5.47 ms。
未复现此前约 230 秒的机器优化停顿，不据此认定旧停顿根因已经修复。

缓存 IR 覆盖 78.0% 的退休指令，每次调用平均 14.79 条；62.00M 次完整检查、
19.48M 次快速检查。helper 退出集中在端口读取 4.90M 次及 CPU control 4.77M 次；
另外有 5.12M 次 interrupt-shadow 退出。合并区域仍有 166 次预算拒绝和 28 次
观察点限制拒绝。下一步应减少这些真实边界的分派次数，并补齐相应的上下文、
raw-host 写入、映射与精确异常证明，不能仅提高区域上限或取消检查。

细粒度计时的外推总量明显偏离实际 batch 时间，报告已给出警告；这里没有把
`estimated_ms` 当作独占 CPU 时间，也没有用接近计时器底噪的范围宣称收益。

随后关闭采样，以现有诊断接口将缓存设为 768 项做单次对照：33.208 秒、
47.012 mIPS、550 次淘汰、约 79.8% 缓存 IR 覆盖及 15.32 条/调用
（`build/ir-sync-xp-capacity768.jsonl`）。淘汰减少，但这一次结果不足以证明
稳定收益，更没有接近 legacy；默认仍为 256。未扩大共享 Wasm table 容量。
