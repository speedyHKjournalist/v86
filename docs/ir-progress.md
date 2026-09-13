# IR-00–IR-14 实施状态

更新：2026-09-13。本轮基于 `ir` 分支提交 `372ccdc42cc8cb66c61c283295ab7ab673b73f2f`；
原始生产基线仍为 `8ee73e538daaab15411344d39a1f271e778ac7f3`。

**完整 IR-00～IR-14 尚未完成。** 本轮实现 IR-11 的受限循环不变量外提（LICM），
并接入优化的 Tier 2 编译。没有切换默认后端，没有退役 legacy，没有将待验收 ISA 标成完成。
[原始实施计划](v86-ir-implementation-plan.md) 的完成条件保持不变。

此前完整进度和逐轮验证记录原样保存在 [实施历史](ir-progress-history.md)。
其中的测试结果属于各自历史版本，不代表本轮已重跑相同的完整系统矩阵。

## 工作包状态

| 工作包 | 当前状态 | 主要剩余工作 |
|---|---|---|
| IR-00 | 部分完成 | 公开优化/调试配置、完整工作负载和测量协议 |
| IR-01 | Builder 改造已实现 | 在线 table 容量仍由运行时策略管理 |
| IR-02 | 部分完成 | 解释器与复杂非法编码共用解码、剩余形式审计 |
| IR-03 | 部分完成；MIR 已独立拥有数据 | 通用 MIR 图变换、剩余动作、分配与栈调度 |
| IR-04 | 部分完成 | 完整 helper registry、隐含状态和异常契约审计 |
| IR-05 | 部分完成 | 剩余整数 ISA、多入口与成熟区域选择 |
| IR-06 | 部分完成 | 通用访存 proof、剩余访存形式 |
| IR-07 | 部分完成 | 在线 REP 调度、远转移和剩余系统/特权指令 |
| IR-08 | 部分完成 | 剩余 SIMD、MMX、FP 控制、F80/x87、无 SIMD 降级 |
| IR-09 | 部分基础 | 完整 Tier 1 语义、区域选择和系统验收 |
| IR-10 | 部分基础 | 跨块 FLAGS/状态同步及更多轻量优化 |
| IR-11 | **部分完成：受限 LICM 已实现** | proof-based 访存复用、forwarding、更多循环与 SIMD 优化 |
| IR-12 | 部分基础 | 完整共享版本/链接图、生产调度策略验收 |
| IR-13 | 完整矩阵未完成 | 完整在线 IR、XP、应用、浏览器与性能矩阵 |
| IR-14 | 未实现 | 覆盖和验收门槛通过后的默认切换与 legacy 退役 |

## 本轮：IR-11 受限 LICM

- 识别自然循环、多回边和嵌套循环；仅使用现有的单一无条件 preheader。
  不创建块、不拆分边，不改变原有预算检查与恢复点。
- 外提经过白名单审计的纯 SSA 标量/向量运算。CPU 状态读取、访存、helper、
  除法、守卫、PollBudget 和有 StateMap/commit 的指令均保持原位。
- 事务式改写：优化预算耗尽时回滚整次变换，并保留原 HIR 继续编译；
  无效 IR 则返回错误。成功后再次运行 verifier。
- 仅在 `config.optimize && request.tier == Tier::Two` 时执行；
  Tier 1 和关闭优化的路径不执行 LICM。统计保存在 `CompiledArtifact.licm`。

实现、安全条件、预算、测试命令及限制见 [LICM 说明](ir-licm.md)。

## 本轮实际验证

测试环境：Linux x86-64，Rust/Cargo 1.98.1，Node.js 22.16.0。

| 检查 | 实际结果 |
|---|---|
| `RUSTFLAGS="-D warnings" RUST_TEST_THREADS=4 cargo test` | **140 passed，0 failed**；其中新增 LICM 测试 13 项 |
| `node tests/ir/wasm/run.mjs` | 总套件通过，包括既有 MIR/SSA/ABI/计数/GVN/边界用例 |
| 新增纯循环 Wasm 对照 | **9,560 次执行通过**；边界值、确定性随机、零次循环和精确预算恢复 |
| 新增 helper 循环 Wasm 对照 | **28,680 次执行通过**；调用顺序、状态观察、异常归属和退出后状态权威性 |
| `cargo check --features ir-experimental` | 通过 |
| 生成器 `--check`、WasmBuilder 输出校验、新增 Rust 格式检查 | 通过 |
| `make ir-default-gate` | **按预期拒绝**：3,728 个生产形式仍为 Pending，不满足默认切换条件 |

本轮本地 `tests/ir/decode/oracle.mjs` 因缺少 `ndisasm` 未完成；
CI 安装 NASM 工具包后运行此检查。新增 CI 配置不等于其检查已通过。
本轮尚未执行完整 CPU 差分、真实浏览器/Worker、XP 启动、游戏负载和性能验收；
没有宣称兼容性覆盖扩大或实际游戏提速。

## 不变的生产门槛

覆盖目录仍为 864 条编码记录、3,830 个粗粒度形式：102 个明确 baseline-UD，
**3,728 个生产形式 Pending**。实验实现能力与生产验收状态分开记录，
本轮没有修改覆盖目录或测试门槛。

`ir-experimental` 可运行 IR 入口及可选自动编译/升档；生产默认仍为 legacy。
完整 ISA、系统/性能验收和旧路径退役继续按实施计划推进。
