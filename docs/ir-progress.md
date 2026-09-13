# IR-00–IR-14 实施状态

更新：2026-09-13。本页记录当前状态，原有逐轮记录完整保留在
[实施历史](ir-progress-history.md)。

**完整请求尚未完成。默认后端仍为 legacy，旧 emitter 未退役。**
公开选择 `ir` 时已有可选自动编译与 Tier 2 升档；这不等同于完整 ISA 或
纯 IR 的 Windows XP、游戏负载和性能验收已经完成。

## 工作包状态

| 工作包 | 状态 | 已落地及剩余工作 |
|---|---|---|
| IR-00 | 部分完成 | 已固定 SHA、保存 release 基线、建立测试入口、编译器配置类型及初始测量记录；已有公开 legacy/ir 后端选择、区域参数、Worker 传递和状态查询；其余公开调试/优化配置及完整工作负载仍待实现 |
| IR-01 | Builder 改造已实现 | u32 locals、完整 LEB 索引/长度、结构签名、类型复用及真实模块执行边界测试；在线 table 容量仍是运行时策略 |
| IR-02 | 部分完成 | 生产区域分析已消费共享 decoded，2,670,035 例与旧 analyzer/EA 解码逐项一致；补齐 helper 块边界及前缀分派 ModRM 读取规则；未知编码仍回退旧分析器，解释器及复杂非法编码模型尚未共用 |
| IR-03 | 部分完成 | HIR arena、SSA、effect、支配/verifier、dump、可执行 dispatcher、边复制和 local 复用；已将标量/RMW 读取和 XMM 访存的 guard、物理动作、CPU 调用与退出策略迁入 MIR 计划；已增加 RMW 提交、地址/范围检查和 SSE 守卫的 effect 计划；已加入 CMPXCHG8B/除法的边界、寄存器与提交计划及通用 helper 调用点的观察/暂存/出口计划；已加入 lowering 拥有的 dispatcher CFG 和 typed 边复制调度；已加入标量/向量值程序、packed kernel 选择及机器栈类型检查；已加入有序状态物化与独立计数阶段，以及 SSA 动态计数基数；已移除保留的 HIR 副本，加入受校验的 lowering 事务、独立 MIR 所有权和机器常量折叠；通用 MIR 图变换、其余动作、入口拆分和栈调度仍待实现 |
| IR-04 | 部分完成 | StateMap、helper 副作用及异常所有权、可执行 outcome ABI；42 例 Wasm 验证状态物化、快照槽位复用和单次派发；已适配 CPU 分段/安全访存及真实 #PF/#GP；已增加终端 CPU 状态 helper 契约；通用 helper registry、更多隐含状态仍待审计 |
| IR-05 | 部分完成 | 寄存器算术、FLAGS、别名、条件码、移位/旋转、SHLD/SHRD、乘除法及终端分支可生成 Wasm；last_op1 和原始 ZF 来源已进入 SSA/快照；已增加位测试/修改、位扫描、POPCNT、BSWAP、XADD、CMPXCHG、BCD、FLAGS 传送、符号扩展和计数分支；已增加可达直接 CFG 字节 lifting；其余整数指令、多入口和在线区域选择未完成 |
| IR-06 | 部分完成 | MOV/moffs/XLAT、整数 ALU/比较/条件操作及 INC/DEC/NEG/NOT 访存；成对 RMW ticket、原生 RAM 和精确 MMU/MMIO；已增加 PUSH/POP、PUSHA/POPA、LEAVE、ENTER 和多次访存提交；已增加交换指令、原生 RAM CMPXCHG8B 及非共享单线程 ABI 下经审计的 LOCK；已增加 FLAGS/段栈操作、段 MOV 和远指针加载；通用 proof 及其余访存待实现 |
| IR-07 | 部分完成 | 近 CALL/RET、FF /2 与 /4 间接转移及动态 EIP StateMap；已增加单次 MOVS/CMPS/STOS/LODS/SCAS 原生执行；已增加标量 IN/OUT 和单次 INS/OUTS；已接入有界 REP HIR、进度映射及最终提交；已增加 CPUID/RDTSC/RDMSR/WRMSR 终端适配；已增加 SYSENTER/SYSEXIT、HLT/CLI/CLTS/WBINVD；已增加 CR/DR 传送与 CPU 地址映射变更适配；已增加描述符表、SMSW/LMSW 与 INVLPG；已增加 SLDT/STR 与 LLDT/LTR；已增加 LAR/LSL、VERR/VERW；在线 REP 调度、远转移及其余特权/系统指令待实现 |
| IR-08 | 部分完成 | XMM V128 SSA、快照、typed locals/边复制，以及 packed/scalar SIMD 传送的原生 RAM 与精确慢路径已实现；已增加 38 种 packed integer 算术/比较/乘法/逻辑及 PS/PD 逻辑别名；已增加打包/解包、变量及立即数 packed 移位；已增加 PSHUF/SHUF 重排；已增加半部传送、MOVD/MOVQ 和重复 lane；已增加符号位掩码、PINSRW/PEXTRW、非临时存储和 LDDQU；已增加 MASKMOVDQU 原生 RAM 与有序慢路径；其余 SIMD 状态/传送、MMX、FP 控制、F80/x87 和无 SIMD 降级仍待实现 |
| IR-09 | 部分基础 | 整数后端可执行 CFG 和寄存器代码；已增加冷 CPU 入口及真实状态 ABI，可执行具备完整动态计数映射的 CPU 循环；CompileRequest 产物已有显式入口键及执行前校验，实验 CPU Wasm 内可直接执行 IR 编译，显式发布的入口已参与正常 CPU 分派；已有可选的自动热度、区域编译和优化升档；完整 Tier 1 语义、成熟区域选择和系统验收仍未完成 |
| IR-10 | 部分基础 | 有界常量折叠、支配关系 GVN、trivial phi 消除、StateMap-aware DCE、常量分支裁剪和保留预算检查的直线块合并；已有独立 MIR 字面量常量折叠；其余跨块 FLAGS/状态同步优化未完成 |
| IR-11 | 部分完成 | Tier 2 已接入保守纯 SSA LICM：自然循环、多回边、嵌套循环、支配约束、工作预算与失败回滚；有原生、独立 Wasm 和实际 CPU/解释器对照；proof-based 访存复用、forwarding、入口拆分、归纳变量及其余 SIMD 优化仍待实现 |
| IR-12 | 部分基础 | 不可变编译请求及 generation/dependency/入口/映射比较已实现；已有只读 CPU 代码快照、单个未发布产物句柄和重校验；共享在线 legacy 桥接已有票据校验、安装前拒绝、缓存取消和浏览器失败回收；已有共享槽池中的 IR 缓存、物理代码页监视和冷执行帧返回后的回收；已有有界自动编译、失败抑制和自动入口淘汰；完整共享版本/链接图及生产策略验收仍未完成 |
| IR-13 | 完整矩阵未完成 | 已执行 IR 差分和部分生产 legacy/Worker/API 回归；GPU 间歇失败、PIC 跳过等结果有单独记录；已有 Node 中显式/自动 IR 缓存分派及升档测试，以及公开后端在真实浏览器主线程/Worker 的升档、SMC、双向跨后端快照和错误上报测试；完整在线 IR、XP、应用及性能矩阵未完成 |
| IR-14 | 未实现 | 默认后端仍为 legacy，旧 emitter 未退役 |

## 本轮：IR-11 纯表达式 LICM

实现与验证入口见 [LICM 契约和回归说明](ir-licm.md)。

- 仅优化经 verifier 认可的 HIR。使用现有无条件 preheader，排除外部入口、
  不可约侧入口和不满足支配约束的循环；支持多回边与由内到外的嵌套外提。
- 使用正向纯操作白名单，绝不把 `ordered() == false` 直接解释成可外提。
  CPU 读取、访存、异常检查、helper、轮询和恢复/提交映射保持原位。
- 工作预算耗尽或末次验证失败时，不提交修改；无可处理循环时不克隆区域。
- 仅在 `optimize && Tier::Two && passes.rounds != 0` 时执行。Tier 1、关闭优化
  和零轮诊断配置保持不外提。统计记录 `passes.loop_hoisted`。
- 新增独立 BigInt/Wasm 对照和在 CPU Wasm 内编译后与解释器逐状态比较，覆盖
  整数溢出、零次循环、预算中断、计数回绕、CS 相对恢复与第二轮缺页。

本轮实际验证结果、命令与未验证范围见 `ir-licm.md`；历史记录中的测试次数、
性能或系统验收不自动计入本轮。初版测试误把入口限定的 `ReadGpr` 放进循环体，
导致三项测试失败；已修正测试并增加拒绝反例，没有放宽 verifier。

## 本轮：IR-13 大型 SIMD 回归的资源上界

现有 packed-integer 与 shuffle 脚本一次持有所有 Wasm 模块，在本轮环境中触发
V8 `Commit wasm code space` 分配失败。已改为按需构造、最多保留 64 组实例的
LRU 缓存，并在淘汰后进行垃圾回收。原始 case 列表、独立算术模型、异常与 MMIO
断言均未删减。新增缓存索引、容量、实例复用和失败重试单元测试。

## 仍未满足的完成条件

生产覆盖目录仍有 3,728 个 `Pending`，默认切换门禁应继续失败。这是覆盖目录的
生产验收状态，不应等同于每个对应编码都没有任何实验性实现。

完整 MMX/浮点/x87、其余 ISA 与系统指令、通用 MIR 图变换、带证明的访存复用、
完整链接/版本管理、XP 与游戏负载、冷/热性能和内存验收，以及旧路径退役仍待完成。
本轮没有加入泛化 `InterpretOne` 伪装完整 lowering，也没有用删除门禁替代验收。
