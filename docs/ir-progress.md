# IR-00–IR-14 实施状态

更新：2026-09-13。固定语义基线：`8ee73e538daaab15411344d39a1f271e778ac7f3`。
本轮续作基于 `ir` 分支 `372ccdc42cc8cb66c61c283295ab7ab673b73f2f`。

**完整 IR-00～IR-14 请求尚未完成。** 本轮推进 IR-11：有界自然循环分析、
事务式纯表达式 LICM，以及优化 Tier 2 的编译入口接入。未切换默认后端，
未移除旧 JIT，未把实验 ISA 能力改记为生产覆盖完成。
原始完成条件仍以 [实施计划](v86-ir-implementation-plan.md) 为准。

## 当前工作包

| 工作包 | 状态 | 已落地与主要剩余工作 |
|---|---|---|
| IR-00 | 部分完成 | 基线、实验配置、公开后端选择和 Worker 接入已有；完整调试配置与工作负载基线待补 |
| IR-01 | Builder 改造已实现 | u32 locals、完整 LEB、签名与边界执行测试已有；在线 table 容量仍属运行时策略 |
| IR-02 | 部分完成 | 生产区域分析使用共享 decoder，已有 analyzer 差分；解释器和复杂非法编码尚未共用 |
| IR-03 | 部分完成 | HIR/SSA/effect/verifier、独立 MIR 所有权与机器常量折叠已有；通用可变 MIR 图、后置分配与栈调度待补 |
| IR-04 | 部分完成 | StateMap、helper outcome ABI 和异常所有权已有；通用 registry 与隐含 CPU 状态仍需审计 |
| IR-05 | 部分完成 | 多类整数、FLAGS、别名和直接 CFG lifting 已有；剩余整数 ISA、多入口与区域选择待补 |
| IR-06 | 部分完成 | 标量访存、栈、成对 RMW、精确慢路径和 CMPXCHG8B 已有；通用 proof 与剩余访存待补 |
| IR-07 | 部分完成 | 近转移、字符串、有界 REP、I/O 及多类系统 helper 已有；远转移与其余特权语义待补 |
| IR-08 | 部分完成 | XMM/V128、整数 SIMD、传送、重排及有序访存已有；MMX、浮点 SIMD、FP 控制、x87/F80 与无 SIMD 降级待补 |
| IR-09 | 部分基础 | 冷 CPU 入口、显式发布、自动编译与升档已有；完整 Tier 1 语义、成熟区域选择和系统验收待补 |
| IR-10 | 部分基础 | 折叠、GVN、phi/DCE、CFG 裁剪/合并及 MIR 字面量折叠已有；跨块 FLAGS/状态同步优化待补 |
| IR-11 | **部分实现** | **新增自然循环分析、现有 preheader 上的事务式 LICM、Tier 2 接入和独立执行对照**；proof-based 访存复用、forwarding、循环重构与进一步 SIMD 优化未完成 |
| IR-12 | 部分基础 | 不可变请求、映射/版本校验、共享槽位、失效、帧后回收及有界自动策略已有；完整共享链接/版本图与恢复策略待补 |
| IR-13 | 完整矩阵未完成 | 本轮 Rust、标准 Wasm 与三类 CPU 构建已验证；完整浏览器/系统、XP、应用与性能矩阵未完成 |
| IR-14 | 未实现 | 默认仍为 legacy；旧 emitter 未退役，生产覆盖门禁仍保留 |

## 本轮新增实现

`analysis/loops.rs` 使用支配关系找回边，合并同一 header 的 latch，按内外层排序；
外部入口、不可约循环、无唯一无条件 preheader 的循环不外提。
`passes/licm.rs` 只移动显式白名单上的全定义、无副作用 SSA 运算，禁止移动
CPU 读取、访存、proof/ticket、helper、除法、SSE 检查、状态观察及预算检查。
预算失败或验证失败不提交候选图；不创建 CFG 节点或改写状态恢复点。

`compile_inner` 只在 `Tier::Two && optimize && rounds != 0` 时运行 LICM，
放在纯数据流优化之后、HIR→MIR lowering 之前。三个编译 API 共用这一入口；
Tier 1 和禁用优化的请求不承担循环变换。产物记录 `loop_passes` 统计。

## 本轮实际验证

| 检查 | 结果 |
|---|---|
| `RUSTFLAGS="-D warnings" cargo test` | **135 passed, 0 failed**，其中 8 项为新增 LICM/循环/Tier 测试 |
| `node tests/ir/wasm/run.mjs` | 标准 Wasm 套件通过，包括 helper ABI、SSA/边复制、动态计数、GVN、MIR 与 Jcc |
| LICM 独立 Wasm 对照 | **45,900 次执行**；未优化、仅 LICM、完整纯优化后 LICM 三路逐状态比较；其中 **4,590 次完成执行**另用 BigInt 算术模型校验 |
| 寄存器与 Builder | Builder 输出核对通过；1,776 组寄存器/FLAGS 分别与解释器、legacy JIT 比较，均通过 |
| CPU 构建 | `v86-ir-test.wasm`、`v86-ir-runtime.wasm`、默认 `v86.wasm` 均构建成功 |
| 构建隔离 | 默认 CPU 的 `ir_*` 导出为零；实验运行时与测试构建分别暴露其自身 IR 导出 |
| CPU 回归子集 | CFG、循环、访存、栈、在线编译、缓存、自动升档、公开后端接入 8 个 make 目标全部通过；含 debug/release/实验运行时适用路径 |
| 完整 `make ir-tests` | Rust 阶段通过后，解码 oracle 因当前容器缺少 `ndisasm` 停止；不能记作整项通过。标准 Wasm 与上述 CPU 子集已分别执行 |
| 生成目录核对 | `generate_ir_decoder.js --check` 通过；864 编码、3,830 形式、3,728 个生产 Pending 未改写 |

以上是本轮隔离 Linux/Rust 1.98.1/Node 22 环境的执行记录，**不是完整 CI、XP
启动、游戏兼容性或性能验收通过的声明**。CI 结果须按提交 SHA 单独检查。
本轮实际执行 `make ir-default-gate` 返回非零，按设计拒绝生产 Pending 非零的默认后端迁移；未绕过该门禁。

## 文档与历史

实现边界、预算和复现命令见 [LICM 说明](ir-licm.md)。本轮之前的完整推进记录已
原样保存到 [实施历史](ir-progress-history.md)，其中测试数字和“下一步”描述属于
各自历史阶段，不应替代本页的当前状态或新增提交的测试结果。
