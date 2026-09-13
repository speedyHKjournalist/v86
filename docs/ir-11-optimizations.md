# IR-11：有界循环外提与精确 SIMD 简化

本次从 `ir` 的 `372ccdc42cc8cb66c61c283295ab7ab673b73f2f` 推进。
固定源码树为 `b90aa889ef9bbd43fc75874d8a464dda50acd3ad`。
**IR-11 仅部分完成，IR-00～IR-14 的完整请求仍未完成。**
本次不改变生产默认后端，不把实验覆盖升级为生产覆盖，不宣称 XP 兼容或游戏提速。
本文件补充基线 [实施状态](ir-progress.md) 中 IR-11 的“未实现”条目：
本分支已推进到下述“部分完成”，历史阶段记录保留不改写。

## 编译入口与统计

`compile_inner` 对 `optimize=true` 的 Tier 2 使用 `passes::run_tier2`；
Tier 1 保留 `passes::run` 的轻量数据流优化。三个 CompileRequest 入口共享此选择。
`optimize=false` 和 `rounds=0` 不运行新增优化；已有 PassConfig 字段和公开后端选项未变。
新增两个 pass 位于 GVN 之后、DCE 之前，最多沿用已有的八轮上限。

- `PassStats.loop_hoisted` 统计移动次数；穿过内外两级前置块的指令计为两次。
- `PassStats.simd_simplified` 统计消除的别名节点及表达式/操作数重写，不是执行频次。
- 单个新增 pass 在预算失败或 verifier 拒绝时保持其输入 Region 不变。
  这不意味着整个 pass pipeline 会回滚此前已经成功的其他 pass；编译失败不会发布产物。

## 自然循环与 LICM

`analysis/loops.rs` 根据现有 CFG 支配关系发现自然循环。同一循环头的多个回边
合并处理；内层先于外层处理，循环头编号打破平局，结果可重现。
必须存在唯一、支配循环头、无条件跳向循环头的前置块。不创建前置块，不拆边，
不修改 CFG，不改变预算检查的数量及位置。外部入口、不可约侧入口和不满足条件
的前驱结构不进行外提；循环参数不会被假设为循环不变量。

`passes/licm.rs` 只接受明确列举的无异常纯 SSA 运算：整数/位运算、选择和位宽转换、
位计数、线性地址的纯算术，以及纯向量位/通道运算。所有输入必须已经在前置块
可用，或按依赖顺序计划先行外提。

**`!Op::ordered()` 不构成可外提证明。** CPU 状态读取、GuestLoad/Store、分段和
权限检查、SSE 检查、除法、RMW、helper、PollBudget，以及携带 state/commit/
故障策略的节点都不外提。计算地址不产生 RAM、页权限或“不为 MMIO”的证明。
即使循环执行零次，提前进行的计算也不得产生可观察副作用或异常。

硬上限为 64 块、8,192 条 arena 指令、16,384 个 SSA 值；工作预算最高 1,000,000，
单次 pass 最多 8,192 次移动。循环发现和移动共用预算。先在块指令表与虚拟归属上
完成全部计划，再提交并验证；提交后验证失败会恢复块顺序与指令归属。
StateMap、effect 链、动态提交计数及退出位置均不改写。

## SIMD 位级恒等变换

`passes/simd.rs` 实现：相同输入的 AND/OR、恒等 shuffle、只选择一个输入的
嵌套 shuffle 合成、extract/replace 往返、相同通道重复写入和不相交通道的读取。
32/64 位同通道写后读可直接引用输入；16 位读取必须保留零扩展，不能把含有高位
的 i32 原样返回。重叠但不完全相同的位区间保持原操作。

变换重写指令、边参数以及仅用于状态恢复的引用，再由类型和支配 verifier 检查。
这里没有浮点代数重排，也不把 ADD、SUB 或 ANDNOT 的相同输入错误当成恒等操作。
已有 SIMD guest 守卫和 CPU 状态初始化规则不变。

## 回归入口

无需磁盘镜像的定向检查：

```sh
node tests/ir/optimization.mjs
```

完整 native/已发射 Wasm 检查：

```sh
env RUSTFLAGS="-D warnings" cargo test
node tests/ir/wasm/run.mjs
node tests/rust/verify-wasmgen-dummy-output.js
```

首次构建前需生成旧分派表；定向脚本和 CI 已包含此步骤。
独立解码 oracle 另需 NASM 包中的 `ndisasm`。

本轮工作容器使用 Rust 1.98.1、Node 22.16.0，已实际完成：

| 检查 | 结果 |
|---|---|
| native Rust 测试（warnings-as-errors） | 139 通过，0 失败，其中新增 12 项 |
| LICM Wasm 对照 | 14,400 次；含零次循环、移位边界、溢出、动态计数和预算恢复 |
| SIMD 简化 Wasm 对照 | 4,096 次；独立位/通道 oracle 与完整状态逐字节比较 |
| 既有 Wasm 执行矩阵与 WasmBuilder 检查 | 通过 |
| legacy / ir-experimental release Wasm 构建 | 均通过，模块验证通过 |
| feature 隔离 | legacy 无 ir_compile_live；两种 release 均无 ir_test_ 导出 |
| 共享 decoder 生成一致性 | 通过，生产 Pending 仍为 3,728 |

本地独立解码 oracle 因缺少 `ndisasm` 未完成，不记为通过。
CI 的 compiler job 安装 NASM 并运行此检查；cpu-runtime job 运行真实 CPU 的 CFG/
循环、缓存失效、自动升档和公开后端集成检查。CI 是否通过以对应提交的实际结果为准，
不能由工作流配置推断通过。

## 未完成的范围

IR-11 的通用地址/页/权限证明、RAM 访存复用、store-to-load forwarding、访存 LICM
及其他循环优化仍未实现。其余 ISA、通用 MIR 图变换、完整版本/链接管理、浏览器/
Worker 全矩阵、XP 和应用/性能验收及 legacy 退役仍须继续推进。
因此不能仅凭本轮优化和测试通过就将 IR 设为生产默认后端。
