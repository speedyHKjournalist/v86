# IR-11：保守、事务式循环不变量外提

本改动继续推进 IR-11，不代表 IR-00～IR-14 完成。生产默认后端、ISA
覆盖清单、默认切换门槛和旧 JIT 均未改变。完整验收仍以
[v86-ir-implementation-plan.md](v86-ir-implementation-plan.md) 为准。

## 接入位置

优化后的 Tier 2 经 `passes::run_tier2`：先执行原有标量/CFG 优化，再执行
`passes::licm::run`，然后进入现有 MIR lowering、MIR 常量折叠和 Wasm 发射。
三个 CompileRequest 入口共用这一路径。Tier 1 与原 `passes::run` 的行为
不变；`IrConfig.optimize=false` 禁用全部优化，`PassConfig.rounds=0` 禁用
HIR 优化及 LICM，但不改变独立 MIR 常量折叠的开关。

`PassStats` 增加 `licm_loops`、`licm_hoisted`、`licm_work`。
`licm_hoisted` 统计移动次数：同一值先从内层循环移到外层前置块，再移出
外层循环时计两次。统计属于编译产物，不改变客户机状态或快照格式。

## 合法性边界

由支配关系识别回边，对同一 header 的所有 latch 求自然循环并集。
只接受已经具有唯一外部前驱、且该前驱无条件跳入 header 的循环；不创建
前置块、不拆边、不修改 CFG。独立入口、不可约循环及缺少合适前置块的
情况不做外提。内层循环先处理，每个循环内部按支配顺序处理定义。

候选必须是白名单内无异常的纯 SSA 计算：整数算术、位操作、转换、选择、
位计数、纯线性偏移和已有纯向量值操作。它必须只有一个结果，不携带
state/commit、fault-trap 或特殊 store 策略。所有操作数必须已在插入位置
可用；循环携带参数不会被当作常量，只有原有 trivial-phi 消除已证明
等价的值才能变成候选的外部输入。

**`!op.ordered()` 不是可外提证明。** CPU 寄存器/FLAGS/段/栈/XMM 读取、
访存、RMW、除法、SSE 守卫、helper、状态恢复点及预算检查都保持原位。
没有访存 proof 的建立、跨 helper 的 load 复用或 store-to-load forwarding。

外提保留 InstId/ValueId，不重写 StateMap/边参数，不移动 effect 链。零次
迭代也可预先执行白名单内的纯计算，但不得新增客户机异常或状态写入。

## 预算与失败原子性

默认最多 262,144 个显式工作单位和 256 次外提；配置上限为 1,048,576 和
1,024。区域上限为 64 blocks、8,192 instructions、16,384 values 和
4,096 StateMaps。工作计数约束扫描，不是墙钟耗时估计；支配分析和 verifier
另受区域尺寸上限约束。

输入首先验证，变换在候选副本上执行，成功并再次通过 verifier 后才替换
原 Region。因此，即使前面的循环或指令已经外提，后续预算耗尽也不会
留下部分变换。零 `max_hoisted` 可禁用单独的 LICM 调用。

## 回归入口

干净检出先生成 opcode 表：

```sh
make src/rust/gen/interpreter.rs src/rust/gen/interpreter0f.rs \
     src/rust/gen/jit.rs src/rust/gen/jit0f.rs \
     src/rust/gen/analyzer.rs src/rust/gen/analyzer0f.rs
make ir-generated-check
cargo test ir::passes::licm
node tests/ir/wasm/licm.mjs
```

Rust 用例覆盖依赖链、非 CFG 顺序 arena、多 latch、自循环、嵌套循环、
不可约/多入口拒绝、恢复专用引用、真实访存 HIR 的有序操作保留、错误输入
与预算回滚，并生成有/无优化的 Wasm。

JS oracle 在九档预算、显式 poll、零次及多次迭代、整数边界值和 FLAGS
组合下比较完整架构输出，同时用独立 BigInt 算术验证完成执行的结果。
无效入口必须保持状态不变。Tier 路由另由 CompileRequest 测试覆盖。

本文件说明测试设计；实际通过情况应以对应提交的 CI 日志为准。

## 未完成范围

尚未实现 proof-based 访存复用、forwarding、归纳变量/强度削弱、SIMD
peephole/向量化、通用 MIR 图变换，亦未完成全 ISA、共享链接/版本图、XP、
应用/性能矩阵或 legacy 退役。没有宣称游戏加载改善、CPU 跑分提升或
纯 IR Windows XP 已通过。候选 Region 克隆与 verifier 的编译开销需要
后续实测；本改动不以删除安全检查换取未经验证的收益。
