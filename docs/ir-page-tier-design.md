# IR 页级基线层（Tier-0）设计

状态：已实现（2026-09-25），默认关闭，用 `ir_tier0: true` 启用；实现与结果见第 7 节。目标：IR 后端在 CPU 基准套件（[cpu-benchmarks.md](cpu-benchmarks.md)）
的每个维度、以及 Windows XP 启动到桌面的时间与平均 MIPS 上都超过 legacy JIT，且不依赖 legacy 编译器。

## 1. 为什么需要新的一层

现有 IR 只有一种编译形态：按入口编译的小区域（平均约 30 条指令），走完整的
SSA、优化、寄存器分配流水线。实测暴露出两个结构性问题：

1. **编译太贵，覆盖率上不去。** XP 启动中 IR 编译约 1.3 ms/区域；legacy 约 1.8 ms/页
   （数百条指令）。按指令算，IR 的编译成本是 legacy 的数倍，只能编译最热的入口：
   覆盖率约 79%（legacy 约 91%），IR 多解释约 1.5 亿条指令，比 legacy 多花约 2.7 s。
   页亲和热度实验把覆盖率提到 84%，但编译量翻倍、激活次数上升，启动时间没有改善。
2. **区域之间的每次转移都回到 Rust。** 基准套件中，即使代码已经完全编译（warm），
   控制流密集的程序也远慢于 legacy：字节码解释器 0.12×、近调用 0.11×、
   600 个函数的大代码 0.16×、虚调用 0.40×。legacy 在同一页内用 `br_table`
   直接分派，任意页内跳转、调用、返回都不离开 wasm。

两个问题的共同解法是一个**编译成本接近 legacy、以页为单位、页内分派不离开 wasm**的基线层。
现有优化编译器保留，只负责真正热的代码（循环、x87/SIMD 数学），这正是它已经快于
legacy 的部分（x87 点积 2.4×、SSE2 整数 1.8×、纯寄存器循环 1.4×）。

## 2. 分层结构

```
解释器 --(页热度)--> Tier-0 页模块 --(入口热度)--> Tier-1/2 优化区域（现有）
```

- 解释器执行冷代码，并按页累计已执行指令数（与 legacy 相同的页热度）。
- 页变热后编译 **Tier-0 页模块**：覆盖该页所有已观察入口可达的指令。
- Tier-0 模块内按入口统计热度；越过阈值的入口交给现有优化编译器，发布后分派优先选优化区域。
- 优化区域的出口若落在已编译页内，下次分派进入该页的 Tier-0 模块，而不是解释器。

## 3. Tier-0 页模块

**编译单元与键。** 一个物理代码页 + 模式标志（32 位代码/栈、CPL3、平坦分段），与 legacy 的
`CachedStateFlags` 相同。一个 wasm 函数，参数为入口序号。

**入口与分析。** 入口集合 = 解释器在该页观察到的块起点 + 页内直接跳转/调用目标 + 页内调用的返回点。
用共享解码器（`ir::frontend::decode`）线性扫描出基本块。跨页的指令、未知编码在该处结束基本块并退出。

**分派。** `loop $dispatch { block ... br_table(next) }`：页内直接跳转、条件跳转、页内 CALL 目标直接设置
`next` 并继续；RET、间接 JMP/CALL 的目标若在本页，经页内偏移表（4 KiB 页、按字节偏移的块号表，
与 legacy 的 `state_table` 相同）查到块号后继续分派；否则写回状态、设置 EIP 退出。

**状态模型。** 指令边界上的架构状态与解释器完全一致：GPR 在 `reg32`，FLAGS 使用解释器的惰性表示
（`flags`、`flags_changed`、`last_op1`、`last_result`、`last_op_size`）。为了性能，8 个 GPR 在函数内
缓存在 wasm 局部变量中（与 legacy 相同），在退出、回退、可能观察状态的慢路径前写回，之后重新加载。

**指令模板与回退。** 为动态频率最高的约 60 种形式写直接模板：MOV/MOVZX/MOVSX/LEA、
ALU/CMP/TEST（寄存器、立即数、内存）、INC/DEC/NEG/NOT、移位/循环移位、IMUL、SETcc/CMOVcc、
PUSH/POP、CALL/RET/JMP/Jcc/LOOP、XCHG、CDQ 等；紧邻的 CMP/TEST/SUB + Jcc 融合为直接比较。
其余指令调用 `ir_t0_step`：写回状态后由解释器执行**一条**指令，返回正常/故障/需要退出，
正常则重新加载寄存器继续。x87/SSE/MMX 先走回退，热点由优化层接手，之后再补模板。

**内存访问。** 模板内联 TLB 快路径（与优化后端相同的 `tlb_data` 检查和主机地址计算）；
慢路径调用可能触发故障的助手，故障时写回指令开始前的状态并退出。写入命中本页代码时，
按现有代码失效协议退出。

**预算与中断。** 每个基本块按指令数扣减执行预算，用尽时写回状态退出，
回到主循环处理中断（与现有 `LOOP_COUNTER` 批次一致）。

**失效。** 页写入走现有 `jit_dirty_page`/IR 页观察路径：失效 Tier-0 模块及依赖该页的优化区域。

## 4. 编译成本目标

- 不建 SSA、不跑优化 pass、不做全局寄存器分配：解码 → 模板直接发射 wasm 字节。
- 目标 ≤ 2 µs/指令（wasm 内），即一页约 0.5–1 ms，不高于 legacy。
- 用基准套件的 cold 分数与 XP 启动的编译时间、覆盖率验收。

## 5. 分阶段计划与验收

| 阶段 | 内容 | 验收 |
|---|---|---|
| M1 | 页分析、页模块骨架、`br_table` 分派、所有指令走 `ir_t0_step` 回退、页热度与发布、失效 | 全部差分测试通过；XP 启动覆盖率 ≥ legacy |
| M2 | 控制流与整数模板、GPR 局部变量、惰性 FLAGS、CMP/Jcc 融合 | 套件 int/control/micro warm ≥ 1.0 |
| M3 | 内存模板（TLB 快路径）、栈、REP 字符串 | memory warm ≥ 1.0；XP 启动快于 legacy |
| M4 | 与优化层衔接：Tier-0 入口热度、优化区域优先、区域出口回到页模块 | x87/sse/mmx warm ≥ 1.0 |
| M5 | 跨页在 wasm 内直连（按页表 `call_indirect`，深度受限） | cold 与 XP 启动进一步提升 |
| M6 | 高频 x87/SSE/MMX 形式的 Tier-0 模板 | 所有类别 warm 与 cold ≥ 1.0 |

每个阶段结束运行 `make bench` 与 XP 启动配对测试，结果写入 `build/bench/`，并与上一阶段用
`tests/bench/report.mjs` 对比，防止回退。

## 6. 风险

- 模板语义必须与解释器逐位一致（FLAGS、故障顺序、段与分页）。每个模板用现有差分框架
  （解释器对照）覆盖，并用随机指令序列测试（类似 `x87_native.mjs`）。
- 页内 GPR 局部缓存要求所有可能观察状态的路径都先写回；回退与故障路径集中实现，避免遗漏。
- wasm 表槽位：Tier-0 与优化区域共享 IR 的槽位池（legacy 在 IR 模式下不占用），需要统一回收策略。

## 7. 实现状态（2026-09-25）

代码：`src/rust/ir/tier0/`（`analysis.rs` 块与布局、`emit.rs` 模板与页函数、`simd.rs` MMX/SSE/SSE2）、
`src/rust/ir/runtime/tier0.rs`（解释器单步、慢路径助手、统计）。启用：构造参数
`jit_backend: "ir", ir_tier0: true`，或导出函数 `ir_auto_set_tier0(1)`。

| 阶段 | 结果 |
|---|---|
| M1 | 完成。页函数、`br_table` 分派、页热度（批量计入）、发布与失效；无模板的指令结束基本块，经共享的单步块由解释器执行一条。 |
| M2 | 完成。整数/控制流模板（含 ADC/SBB、ROL/ROR、SHLD/SHRD、BSF/BSR、BT*、MUL/DIV、XADD/CMPXCHG、LOCK）；GPR 常驻局部变量；惰性 FLAGS **延迟写回**（块内被覆盖的 FLAGS 不写内存）；CMP/TEST+Jcc 与未知生产者的条件都内联求值；页内强连通分量编译为嵌套 wasm `loop`，返回点在循环分派处直接比较。 |
| M3 | 完成。TLB 快路径；慢路径助手；平坦分段/32 位栈特化（入口检查）；写入含 IR 代码的页时交给解释器（无需写后检查）。REP 字符串走单步。 |
| M4 | 未做，也不再需要：x87/SSE/MMX 直接由 Tier-0 原生模板覆盖，已快于优化层。 |
| M5 | 以 Rust 辅助方式完成：页函数退出时 `ir_t0_chain` 查页见证后嵌套调用下一页函数；CPU 循环对页见证走精简激活路径（`t0_execute`）。相邻页合并（一个函数覆盖 2–3 页）已实现但默认关闭：XP 启动链接只减少 5%，代码量增加 38%，启动变慢 7%。 |
| M6 | 完成。x87 复用优化层的 f64 原生发射器（`backend/wasm/x87.rs`，泛化为 `X87Words`）+ `ir_t0_x87` 慢路径；x87 的 TOP/tags/VALID/DIRTY 在整个页函数内缓存在局部变量中（运行时“已打开”标志，在读 x87 状态的指令和函数出口前写回）；连续 ≥3 条纯寄存器 x87 指令按相对栈位置编译为一段 f64 局部变量代码（`tier0/x87run.rs`，一次入口检查、一次提交）。MMX/SSE/SSE2 用 wasm SIMD 原生实现，块内 XMM 寄存器缓存在局部变量中；NaN 结果交给解释器以保证逐位一致。 |

**结果**（`build/bench/results-tier0.json`，满规模，所有校验和与指令数与 legacy 一致）：
总分 warm 1.41×、cold 1.32×（优化区域层为 0.55×）；x87 2.45、SSE 2.34、MMX 1.53、micro 1.16、
int 1.07、memory 1.04、control 0.96。低于 legacy 的：708.pages 0.44（legacy 一个模块覆盖多页，
Tier-0 每次跨页都要链接）、502.codebloat 0.86（同样是跨页）、vcall 0.93、bytecode 0.94，
其余（recursion、muldiv、rmw、trig）在 0.97–0.99 的测量噪声范围内。
**XP 启动到桌面**（同步磁盘、交替测量）：Tier-0 约 12.1–12.4 s，legacy 约 13.0–13.2 s。

**正确性。** `tests/ir/differential/tier0_fuzz.mjs`：随机整数/SSE/x87 程序在循环中运行到页被 Tier-0 编译，
与纯解释器比较 GPR、EFLAGS（用 PUSHFD/LAHF 记录 AF/OF）、内存、XMM、x87 状态与指令数；
`FUZZ_KIND=i0..i33|s0..s9|x` 单独测试某一类。

**踩过的坑。** IR 编译器的 Rust 分配使 2 GiB 的 wasm 内存反复 `memory.grow`，每次都触发 V8
“external memory pressure” 全量 GC（XP 启动约 170 次、0.8 s），启用 Tier-0 时一次性预留编译堆解决；
标量 SSE 先 8 字节写 XMM 再 16 字节读会导致存储转发失败，标量形式只读用到的通道。
