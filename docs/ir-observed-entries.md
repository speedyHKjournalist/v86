# Tier 1 共享已观察侧入口

自动调度原先只把达到独立编译热度阈值的相邻入口合并到同一代码体，并按每个入口到
快照末尾的 suffix 长度累计收费。这会拒绝多个相互重叠、实际只需一个共享代码体的
入口；某个已观察的冷入口即使位于刚编译区域内，也可能继续解释执行。

现在仅对 Tier 1 的共享编译尝试放宽这两个条件：

- 主入口必须仍按原规则达到编译热度，并消耗原有的每帧编译 credit。
- 最多选择 3 个已经观察过、hits 大于零的侧入口。它们必须仍在有界热度表内、位于
  同一个不可变快照内、拥有相同 CS base 和默认位宽、尚未发布，也没有该 tier 的
  已知编译失败。排序仍按热度降序、地址升序，选择结果可重复。
- 共享尝试只有一份代码快照，不再对重叠的 suffix 重复累计输入字节。快照长度、
  CFG/HIR/MIR 工作预算、执行预算和侧入口数量上限继续生效。
- 共享失败时，独立 fallback 重新使用原有热度阈值和累计 suffix 字节限制。冷侧入口
  不会变成独立编译任务；没有合格热侧入口时只编译主入口。Tier 2 的选择规则不变。

选择逻辑位于 [peers.rs](../src/rust/ir/runtime/peers.rs)，在线调度仍由
[schedule.rs](../src/rust/ir/runtime/schedule.rs) 执行。此策略不增加热度表容量，也不
把未观察的指令边界自动注册为入口。

## 入口与生命周期契约

复用现有 `compile_cpu_shared_entries`：前端独立检查每个入口的指令边界，重叠 x86
指令流会拒绝共享。每个外部入口都从 CPU backing state 建立 SSA 状态；不继承另一
入口的寄存器或 FLAGS 假设。生成模块仍拒绝错误 PC、CS、默认位宽、prefix、HLT 状态
和非零初始 selector，然后才执行入口初始化。

成功共享只有一个 cache record、Wasm slot 和不可复用的 owner ID。所有别名经过
同一发布事务及代码/映射验证；pending 或取消的别名没有执行权限。执行任何别名前
仍验证整个 owner 的源代码，原始 host 写、已通知代码写、映射变化、reset、替换和
回收不能通过侧入口绕过失效。CPU 精确异常、预算、退休计数及观察者契约均沿用原路径。

发布成功会同时重置实际发布入口的热度。Tier 1 别名随后必须通过真实激活重新积累
promotion 热度；没有给冷别名补造访问次数。别名单独升 Tier 2 时仍可拆出新 owner，
原共享 owner 在还有其它入口使用时保留。

## 成本与适用边界

共享源字节不等于共享编译或执行没有额外成本。每个侧入口增加上下文检查、外部
入口状态、可能的 phi/local 活跃区间；还可能带入主入口不可达但位于快照内的代码。
共享尝试失败后会再次编译主入口和合格热 suffix，因此额外工作虽受预算限制，仍可能
在代码 cache 反复淘汰时重复发生。目前没有单独缓存失败的共享组合。

此变更不调整 Tier 2 的调度优先级或每帧 credit。共享可以节省后续独立 Tier 1 工作，
但更多实际执行的别名也可能增加以后 promotion 和 cache 的压力。128 项热度表已经
遗失的地址不会因本策略被恢复；不能据此承诺解决整个 XP 工作集问题或达到 legacy
性能。最终取舍需要关闭诊断的同批 XP 配对，而非只比较别名发布数。

## 验证

`ir::runtime::peers` 的两项原生测试检查数量上限、确定性热度排序、完整入口上下文、
4 GiB 地址回绕、冷入口只参与共享，以及 fallback 的原热度/累计字节预算。当前原生
完整测试通过 282 项、失败 0 项、忽略 4 项，其中包含这两项测试。

[observed_entries.mjs](../tests/ir/differential/observed_entries.mjs) 接入 `ir-auto-tests`，
检查只访问一次的侧入口不能自己触发编译，主入口达到阈值后才提交共享代码体；并
检查共享 slot、实际从别名执行 IR、未观察入口仍不可调用、重叠指令流回退保留主入口、
没有独立冷 sibling 排队、准确的回绕退休计数，以及取消和原始字节改写的发布拒绝。

固定旧核心 `build/ir-next-baseline.wasm` 已复现该回归的预期失败：主入口成功发布，
但观察过一次的侧入口仍不存在（期望 1，实际 0）。此前的冷入口和精确 192 条退休
检查通过。新核心的运行测试及 XP 结果由本轮统一验证记录提供，不把旧核心失败本身
视为新实现已经通过验收。

stage3 的 cache debug/release 核心均通过新增 4 个在线侧入口案例，同时通过
`scheduler_ready`、`missing_hint` 和 13 组配对 `warm_chain` 回归。runtime 核心通过
完整 cache、auto、diagnostics 和 20 个 I/O permission observer 案例；自动调度测试
记录 611 次发布。该批 12 个独立测试进程全部退出 0，日志保存在
`build/ir-next-stage3-lifecycle-*.log`。这些是功能和生命周期证据，XP 性能仍以统一
关闭插桩的配对测量为准。

后续包含 FLAGS/CPU edge 修复的 stage4 三个核心再次通过同一组 12 个测试进程，
日志为 `build/ir-next-stage4-lifecycle-*.log`。测试前保存三个核心的 SHA-256，且每个
测试开始前再次检查文件未变；校验值见 `build/ir-next-stage4-lifecycle-cores.json`。

```sh
RUSTFLAGS='-D warnings' cargo test ir::runtime::peers
node tests/ir/differential/observed_entries.mjs build/v86-ir-runtime.wasm
make ir-auto-tests
```
