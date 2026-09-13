# IR-00–IR-14 实施状态

更新：2026-09-13。续作基线：`372ccdc42cc8cb66c61c283295ab7ab673b73f2f`；
固定旧实现参考：`8ee73e538daaab15411344d39a1f271e778ac7f3`。

**完整请求尚未完成。** 本轮实现了 IR-11 的保守循环不变量外提和精确 SIMD
改写，并修复大规模差分测试的 Wasm 产物常驻问题。没有切换默认后端，
没有退役 legacy，没有把实验性 lowering 或测试通过等同于生产覆盖完成。

原始完成条件仍以 [实施计划](v86-ir-implementation-plan.md) 为准。
此前全部阶段记录完整保存在 [历史进度](ir-progress-archive-20260913.md)；
其中的测试数量与状态属于对应历史阶段，不是本轮重新执行的声明。
本轮实现、验证命令和限制见 [IR-11 优化说明](ir-11-optimizations.md)。

| 工作包 | 当前状态 | 已实现与剩余范围 |
|---|---|---|
| IR-00 | 部分完成 | 固定基线、配置类型、公开后端/区域预算/状态接口已有；完整工作负载和调试配置仍待完成 |
| IR-01 | Builder 改造已实现 | u32 locals、LEB、类型/签名及真实 Wasm 边界测试；在线容量仍属运行时策略 |
| IR-02 | 部分完成 | 共享 decoder 已进入生产区域分析；未知编码回退、解释器共用及复杂非法形式仍待完成 |
| IR-03 | 部分完成 | HIR/SSA/effect、独立拥有数据的 MIR、机器计划和 MIR 常量折叠已有；通用 MIR 图变换、后置分配及栈调度仍待完成 |
| IR-04 | 部分完成 | StateMap、helper outcome/异常所有权及多类 CPU 适配已有；通用 registry 和隐含状态审计仍待完成 |
| IR-05 | 部分完成 | 多类整数算术、FLAGS、位操作、移位/乘除及可达 CFG lifting；其余 ISA 与多入口仍待完成 |
| IR-06 | 部分完成 | 原生 RAM、MMU/MMIO、RMW、栈与段访存已有；通用 proof 和其余访存仍待完成 |
| IR-07 | 部分完成 | 近控制流、REP、I/O、多类 CPU/特权 helper 已有；远转移、其余系统语义和调度仍待完成 |
| IR-08 | 部分完成 | XMM SSA、传送、packed integer、shuffle、lane 和部分 masked 访存已有；MMX、FP 控制、F80/x87 和无 SIMD 降级仍待完成 |
| IR-09 | 部分基础 | CPU Wasm 内编译、正常分派入口、可选自动 Tier 1/2 已有；完整 Tier 语义、区域选择和系统验收仍待完成 |
| IR-10 | 部分基础 | 有界 folding/GVN/DCE/phi/CFG 优化及 MIR 折叠已有；更多跨块 FLAGS/状态同步优化仍待完成 |
| IR-11 | 部分完成（本轮推进） | Tier 2 纯表达式 LICM、精确整数向量/shuffle/lane 改写已实现；proof-based 访存复用、store forwarding、更多循环优化仍待完成 |
| IR-12 | 部分基础 | 共享缓存/槽位、版本校验、代码页失效、执行后回收和可选自动策略已有；完整版本/链接图和生产策略验收仍待完成 |
| IR-13 | 完整矩阵未完成 | 本轮 139 项原生测试、独立 Wasm oracle、三套 SIMD CPU 差分及缓存/升档/后端回归通过；完整 ISA、真实浏览器/Worker 重验、XP、应用和性能矩阵仍待完成 |
| IR-14 | 未实现 | 默认后端仍为 legacy，旧 emitter 未退役 |

## 本轮验证结果

`RUSTFLAGS="-D warnings" cargo test`：139 项通过。Wasm runner 通过，包括新增
12,336 次 LICM 独立循环/预算模型执行和 38,912 次 SIMD 独立字节/lane 模型执行。
三套现有 SIMD CPU 差分完整执行 debug/release，未删减案例：integer、shuffle、lane。
缓存和自动升档分别通过 debug、release 和无测试钩子的 IR runtime 构建；
Node 公开后端回归通过。debug/release 缓存测试各包含 900 次真实 legacy 发布/淘汰。

本地独立 NDISASM oracle 因缺少 `ndisasm` 未执行成功；新增 CI 安装 `nasm`
并执行该检查。原生 decoder 测试和生成代码一致性检查已通过。
CI 的最终结果应查看实际 workflow，不能由本地结果推定。

## 默认切换门禁与下一依赖

覆盖目录仍为 864 条编码记录、3,830 个粗粒度形式，含 102 个 baseline-UD 形式；
**3,728 个生产形式仍为 Pending**，本轮没有改写覆盖状态。
`make ir-default-gate` 应继续拒绝默认切换。

`jit_backend: "ir"` 是显式选择；可自动编译/升档且禁用 legacy 生成，但未支持
语义仍由解释器执行。这不等于完整 ISA 或 legacy 退役。没有 XP 兼容或性能收益声明。

下一依赖仍是补齐 IR-02/03/04 契约、未覆盖 ISA、完整链接/版本管理，完成
XP/应用/性能验收，最后再决定默认切换和旧路径退役。具体既有实现见
[后端配置](ir-backend.md)、[MIR 所有权](ir-mir-owned.md)、
[覆盖说明](ir-coverage.md) 和 [历史测试报告](ir-validation.md)。
