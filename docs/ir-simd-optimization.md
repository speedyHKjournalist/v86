# IR-11：有界 SIMD SSA 化简

本增量基于 `ir` 的 `372ccdc42cc8cb66c61c283295ab7ab673b73f2f`，
实现纯向量表达式化简。它不是 IR-11 或 IR-00～IR-14 的全部完成声明。
生产覆盖表、默认后端、解释器回退和 legacy emitter 均保持不变。

## 已实现的变换

| 模式 | 行为与限制 |
|---|---|
| 恒等 shuffle | 精确选取同一输入的原始 16 个字节时，结果别名为该输入 |
| 嵌套 shuffle | 按被选字节追踪一层来源；最终只需要至多两个不同 SSA 向量时合并，循环处理可继续消除更深重排 |
| 相同输入 shuffle | 将重复来源规范化，不依赖寄存器编号或 CPU 状态读取的值相等假设 |
| extract(replace(v, x)) | 同宽同通道的 32/64 位读取转发 x；16 位读取必须重写为 `x & 0xFFFF`，保持截断和零扩展 |
| extract(replace(v, x))，不同通道 | 只有两个字节区间完全不重叠，才绕过写入；混合宽度的部分重叠不化简 |
| replace(replace(v, x), y) | 同宽同通道的后写入覆盖前写入；不移除其他观察点需要的 SSA 定义 |
| replace(v, extract(v)) | 同宽同通道的写回别名为 v |
| extract(shuffle(a, b)) | 所选字节必须来自一个输入、连续且按目标通道宽度对齐，才直接读取对应输入通道 |
| v AND v / v OR v | 位精确别名化；不把 XOR、ANDNOT 或其他 packed 运算错误地当成恒等式 |

每条规则仅处理无 `state`、无 `commit`、无故障后 trap 和无特殊存储标记的
单结果纯向量节点。CPU 读取不参与值相等推断。访存、MMIO、SSE 检查、helper、
effect 链、预算轮询、控制流边和客户机指令计数都不移动。

## 接入与安全约束

实现位于 `src/rust/ir/passes/simd.rs`。`PassConfig::simd` 是独立开关，默认在
已有 HIR 优化管线中启用，位于 GVN 和 StateMap-aware DCE 之前；关闭整体优化、
关闭此开关或 `rounds = 0` 不执行此 pass。这不改变 V86 的默认 CPU 后端。

管线先快速筛选候选：整数代码和无关 packed 运算不进入克隆、支配分析及额外
verifier 流程。候选按支配顺序处理，不做跨块代码移动；别名会同时更新指令参数、
分支参数及全部恢复映射中的引用。新建的 16 位 mask 常量紧邻重写的原指令之前，
保留原结果 ID 和 I32 类型，后续 GVN/DCE 可清理重复常量和死向量表达式。

显式调用 `simd::run` 会验证输入，在私有副本上变换，验证输出后才整体提交。
输入无效、工作预算耗尽或 arena 增长超限时，原 Region 的 arena、调度、恢复映射
全部保持不变。上限为 64 blocks、8192 instructions、16384 values、8192 states 和
1024 helpers；默认工作预算为 1,000,000。变长参数、状态、helper 元数据也计入预算。
固定 arena 上限约束 verifier 与克隆；work 不是宿主 CPU 指令数或耗时指标。

`Stats` 分别记录别名数、重写数、shuffle/lane/bitwise 命中及工作量；这些是规则
执行次数，不等于唯一指令数或性能提升。管线累计 `PassStats::simd_simplified`。

## 可复现验证

```sh
make ir-simd-opt-tests
RUSTFLAGS="-D warnings" cargo test
node tests/ir/wasm/run.mjs
node tests/rust/verify-wasmgen-dummy-output.js
```

专项目标自动生成 Rust 编译依赖的 CPU 指令表。新增 11 项 Rust 测试覆盖规则、
StateMap-only 使用、分支参数、多入口隔离、SSE effect/恢复点固定、开关隔离、
预算边界及发生过别名化之后的 arena 增长失败原子性。

执行语料包含 868 个表达式 DAG、68 组确定性输入和四种配置：不优化、只运行新
SIMD pass、完整管线启用 SIMD、完整管线关闭 SIMD，共 236,096 次 Wasm 执行。
JS 模型从**优化前**的表达式 DAG 按字节与 BigInt 独立求值，不复用 MIR/Wasm
重排函数；核对 GPR、XMM、EFLAGS、EIP、计数，并逐字节比较前 4096 字节 CPU
状态 backing。输入覆盖高位脏数据、所有受支持通道宽度、部分重叠、未对齐重排、
三来源无法融合、多层共享表达式、计数和 CS 基址回绕。

该 corpus 的两个完整管线仅 SIMD 开关不同，生成模块总大小为
838,849 → 819,458 字节。这是固定合成语料的代码大小，不是运行时间、游戏加载
性能或 Windows XP 验收结果。这里的 CPU ABI 纯表达式测试使用受控导入，不能
替代真实 CPU/MMU 的异常、系统或应用矩阵。

新增 `.github/workflows/ir-simd.yml` 分别执行编译器/独立模型和真实 CPU 差分。
实际远端结论应查看对应提交的 Actions 结果，不能仅凭 workflow 列出了目标就
宣称全部通过。此前一般仓库 CI 的失败也没有被关闭或绕过。

## 尚未实现或验收

不包含 proof-based 访存复用、store/load forwarding、访存 LICM、其余循环优化、
完整 SIMD/FP/MMX/x87 ISA、通用 MIR 图变换、完整链接/版本管理、Windows XP 与
实际应用/性能矩阵，也不切换默认 IR 或退役 legacy。原始总体验收标准见
[v86-ir-implementation-plan.md](v86-ir-implementation-plan.md)。
