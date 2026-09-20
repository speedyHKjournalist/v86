# 共享解码、CPU 契约、系统模式与 FP 验收增量

2026-09-19；在 `e87c1f43270a0d3185ed48e40dbf9069a60360a6` 上继续实现。
本页记录本次四项增量；不替代 IR-00～IR-14 的 XP、应用、性能及旧后端退役验收。

## 共享解码与非法形式

`decode_rules.rs` 被快照 decoder 和分阶段 CPU interpreter 同时使用：
前缀累积、最后一个段前缀、mandatory-prefix 优先级、16/32 位 ModRM/SIB
地址与默认段选择均只有一份规则。解释器保持原有的取指时机，尤其保留
SIB 带位移形式先检查段、再读取位移的基线故障优先级。

目录现在有 **935 个编码、3,972 个粗粒度形式**。新增 71 个缺失 `/g` 选择，
它们读取 ModRM 后直接 #UD，不读取 SIB、位移或立即数。244 个 BaselineUD
形式有显式 CPU 终端 lowering，按原有顺序执行 TS/EM 和段检查，不读取非法
形式的操作数内存。debug 下重复 REP/不支持的 mandatory-prefix 断言以及
`0F AE /2 reg` 的断言仍是编译停止；release 语义不伪装成 debug 语义。

验证：2,678,271 组 analyzer 对照、每种核心 52,592 组解释器地址解析对照
（独立固定旧 resolver，含跨页取指/空段优先级）、每种核心 3,776 组非法形式
CPU 对照。前缀乘积和每个截断点另外在 Rust 测试中验证。

## MIR、低层 import 与入口

`helper/imports.rs` 集中登记 31 个低层 import 的固定 Wasm 签名、返回协议和
隐含状态。MIR 封装及发射前校验参数类型、结果类型与消费协议；内存、RMW
票据、CPU 自有故障、终端写入和向量慢路径不能相互冒充。发射器的固定入口
调用也使用同一签名表。已有 HIR effect/RMW 仿射约束、RAM guard/forwarding
证书、StateMap 和 lowering 事务校验继续适用，未增加未经证明的跨故障重排。

返回式 FP helper 的 continuation 审计补入 TR/LDTR 缓存、GDTR/IDTR、TSS 宽度
以及全部非算术 FLAGS；这些状态或既有模式/页表/代码上下文发生变化时，
提交已完成指令并冷退出，不进入后续 SSA。测试覆盖 GPR/算术 FLAGS/XMM 重载
与描述符、任务缓存、控制 FLAGS 变化后的退出。

`compile_cpu_entries` 接受同一不可变快照上的最多 8 个入口，拆成分别带 CPU
入口守卫的 artifact。支持重叠 x86 指令入口；各自裁剪映射和物理依赖。
重复入口、job/slot 冲突、不同 VM generation、越界和任一编译失败会使整个
调用返回错误，调用者不会拿到部分成功结果。发布仍须逐 artifact 校验版本；
该 API 本身不安装缓存。新增实际 Wasm 执行验证重叠入口、精确计数和错误入口
在任何状态写入前退出。

## 系统模式与精确异常

新增每种核心 **172 组**优化/未优化对照：VM86 高/低 IOPL 的远转移、INT 和
IRET，ring 0 → ring 3 IRET，IRET 返回 VM86，16/32 位调用门，ring 3 中断，
32 位任务 CALL/JMP/IRET/任务门以及任务进入 VM86。逐返回帧访问制造跨页 #PF，
覆盖调用门目标栈预检查、任务旧 TSS 写故障和新 TSS 读故障。

同时运行当前 debug/release 和固定 `90f90481…` 控制语义的 reference 核心。
比较寄存器、FLAGS、段隐藏缓存、CPL、模式、CR、GDT、旧/新 TSS、异常栈以及
IR 单次退休。固定基线的部分任务切换错误仍在交付异常后 `unwrap` 中止；
测试明确比较中止和部分写入，不将其计作完整硬件任务切换支持。16 位 TSS、
未实现的 gate jump/异常处理及其他基线 panic 分支仍不属于新增支持。

## FP 特殊值、便携核心与 helper 审计

SSE FP 每种核心 **88,688 组**用例包括 F32/F64 正负零、subnormal/normal 边界、
极值、无穷、带 payload 的 qNaN/sNaN、转换/舍入边界、四种舍入、DAZ/FTZ
和异常 mask 开关，继续比较原始位模式与 MXCSR。x87 每种核心新增 **99,840**
组寄存器 F80 特殊值/精度/舍入和 **4,608** 组内存特殊值/控制模式用例。
这验证与 CPU 基线一致，不声称修复基线未实现的 FP exception/DAZ/FTZ 行为。

测试发现 LLVM 在不同内联路径中选择不同 NaN payload（debug ADDSUBPS、release
ADDPS）。IR-enabled core 中 68 个 SSE FP 共同语义入口使用 `inline(never)`，
让解释器与 IR 调用同一实际 kernel；正常生产核心的内联策略保持不变。
这保留额外函数边界成本，不能当作 FP 性能优化或历史构建 NaN payload 固定保证。

新增 `v86-ir-runtime-fallback.wasm` 和测试便携核心，以 `-simd128` 构建。
便携 IR 编译器在发布前拒绝 V128 artifact，标量区域仍正常编译/升档；guest SIMD
由解释器执行，失败缓存抑制重复编译，IR 模式不调用 legacy 编译器。
主线程和 Worker 支持 `wasm_fallback_path`；自定义核心文件名的默认 fallback
保留 URL 查询参数和 fragment。CI 使用可控 primary CompileError 验证加载重试，
并实际运行便携核心；此测试宿主支持 SIMD，未冒充真实无 SIMD 浏览器实测。

`helper_audit.mjs` 检查各优化 artifact 的 import，并输出
`build/ir-helper-audit.json`。常用 packed integer、移动、shuffle、lane 形式没有
FP/MMX 算术 helper 导入；保留的 FP/MMX 调用按家族列出。该记录是 ABI/模块
体积审计，不是耗时基准。尚未以 native FP 指令替换会影响 NaN/舍入/状态语义的 helper。

## 可重复入口

- `RUSTFLAGS='-D warnings' cargo test --lib`
- `make ir-decode-contract-tests ir-system-mode-tests ir-portable-tests`
- `make ir-x87-tests ir-x87-memory-tests ir-sse-fp-tests ir-helper-reload-tests`
- `make ir-helper-audit`

生产 Pending 仍为 3,728，实验粗粒度 Pending 为 0。完整形式目录不是所有
前缀长度、系统状态和设备回调组合的穷举证明；默认后端切换、XP/应用长程测试、
性能目标和 IR-14 旧路径退役仍须按原计划独立验收。

本地最终结果：218 项 Rust 测试（warnings-as-errors）通过；上述解码/非法形式/
多入口/系统模式门禁、debug/release FP 矩阵、固定旧控制语义对照、便携核心与
自动发布/升档/失效/恢复生命周期测试通过。诊断日志保存在 `build/ir-*.log`。
低层标量读/RMW 继续遵守其既有 callback ABI（保持缓存的 CPU 架构状态）；
新增完整重载和上下文变化退出保证适用于显式 `CpuReload`，不扩张成任意回调重入保证。
