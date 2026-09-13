# 有界纯 SSA 循环与值优化

本轮以 `372ccdc42cc8cb66c61c283295ab7ab673b73f2f` 的 IR 编译器为基础，
推进 IR-10 与 IR-11。没有宣布全 ISA 或生产迁移完成。

## 管线与开关

`passes::run` 在常量折叠之后、CFG 裁剪/GVN 之前执行 `canonicalize`，
在 GVN 之后、DCE 之前执行 `licm`。`PassConfig` 中两个同名布尔字段均可独立关闭。
默认优化配置启用它们；是否优化仍由已有编译请求与 Tier 策略决定。
正常默认后端仍为 legacy，自动 IR Tier 1 仍不启用优化，Tier 2 消费默认 pass 配置。
未修改 CPU 布局、快照、graphics proxy、CPUID、客户机计时或异常 ABI。

`PassStats` 增加 `canonicalized`、`simd_simplified`、`loops`、`hoisted`。
统计为各轮累计变换/访问次数；嵌套循环中的同一表达式可能被逐层外提，
同一循环可能在不同轮次再次统计，不应解释为唯一循环数或运行时热度。

## 标量与 SIMD 规范化

`src/rust/ir/passes/canonicalize.rs` 按支配顺序扫描显式 SSA 定义。
规范化处理加减零、乘零/一、位运算恒等式、自比较、select、交换律排序、
等宽 extend/truncate 往返，以及可证明包含/不相交的 extract/insert。
移位计数遵循当前 HIR 的 i32/i64 机器掩码 31/63，而非窄操作数位宽。
不使用浮点交换律、NaN 或 signed-zero 假设。

SIMD 处理 AND/OR 自运算、lane 恢复/覆盖/提取以及 shuffle 恒等式与单层组合。
16 位 lane 的插入操作数是 i32，读回必须零扩展，不能直接替换为可能有高位的
插入操作数。只在相同 32/64 位 lane 或其他满足精确字节语义的情形建立别名。
当前 HIR 只支持 16/32/64 位 lane 操作，本轮未扩展为 8 位 lane。
嵌套 shuffle 仅在最多需要两个底层向量源时组合；三源保留。

别名改写涵盖普通输入、边参数、条件和全部 StateMap 值（包括仅恢复时可见的值）。
不删除 guest load、helper、预算点或状态观察，仅其无用计算结果可以被 DCE 回收。
该 pass 限制 64 blocks、8192 instructions、16384 values 和 65536 输入参数；
外层优化轮数上限仍为 8。统计不等同于运行速度测量。

## 事务式 LICM

`src/rust/ir/passes/licm.rs` 合并同一 header 的所有 latch 来识别自然循环，
要求 header 支配所有成员、无其他外部入口，并存在唯一、无条件跳往 header 的
前置块。拒绝外部入口 header、条件前置块、多个前置块及不可约侧入口。
不新建或拆分 CFG，不插入新的 guest 入口。按内层到外层及支配深度确定扫描顺序。

只移动经过白名单审计的全定义、无副作用表达式，且其输入必须在循环外定义并
支配前置块。纯算术与向量值操作可以投机执行；CPU backing-state 读取、分段地址
解析、内存、SSE 守卫、helper、可能出错的除法、RMW、状态物化与 PollBudget 均不移动。
`!ordered()` 不是可移动性的充分条件。未建立任何 RAM/MMIO 或页映射 proof。

规划只更改指令归属和块内顺序，保持稳定 ID、effect 链、恢复图、边参数和
客户机计数不变。默认最多 2,000,000 单位规划工作、4096 次外提，
并在 verifier 前限制 arena 大小。work 耗尽返回错误且 Region 不变；hoisted 上限
允许成功返回保守的部分优化。候选 Region 验证成功后才整体替换输入。
这些原子性保证针对 LICM 单次调用，不宣称整个既有 pass 管线是事务式的。

## 验证入口

```sh
make ir-licm-tests
make ir-optimization-tests
make ir-tests ir-cfg-tests ir-memory-tests
```

第一项会生成 baseline dispatch tables，然后执行 LICM Rust 测试和真实 Wasm oracle。
第二项还构建 IR test CPU，执行标量与 SIMD 规范化 oracle。
完整测试依赖 Rust、Wasm target、Node、NASM/NDISASM 及仓库原有构建工具。

新增 15 项 Rust 测试覆盖多回边、自循环、嵌套循环、不可约图、外部入口、
预算失败原子性、外提上限、CPU 观察、恢复-only 值、窄位宽和三源 shuffle 负例。
一次完整 `RUSTFLAGS="-D warnings" cargo test` 通过 142 项测试。

独立预期值计算而非仅比较同一编译器的两个版本：

| 新增执行组 | 实际执行次数 | 核查内容 |
|---|---:|---|
| LICM 循环 | 12,150 | 零次迭代、回边、整数溢出、九种预算、恢复状态和非法入口 |
| 标量规范化 | 57,024 | 五种 SSA 位宽、机器移位掩码、子寄存器、FLAGS、计数 |
| SIMD 规范化 | 16,380 / CPU 构建 | 独立字节语义、全部支持 lane、窄高位、shuffle、真实 CPU ABI |

上述每组包含不优化、只执行目标 pass、完整管线三个模式。
真实 CPU SIMD 测试中不执行 guest 解码/MMIO；相应访存/异常由既有差分套件另行核查。

## 明确保留的边界

未实现 proof-based 访存复用、store-to-load forwarding、内存 LICM、循环展开/
强度削减或普遍 SIMD/FP 优化；未补完 MMX/x87/远控制转移和其他 ISA。
未迁移全部生产 Tier，未删除旧 emitter，未声称 XP、游戏加载或 FPS 提速。
现有 coverage gate 和 [完整实施计划](v86-ir-implementation-plan.md) 不变。

## 大型差分 corpus 的资源上界

两个既有 SIMD 测试曾一次性编译并持有全部 case 的两个版本（整数 33,120 组、
shuffle 29,400 组），会在有限映射数量环境中耗尽 Wasm code spaces。
现在按访问顺序惰性实例化，最多缓存 32 对实例。LRU 只影响测试缓存生命周期，
所有 case、debug/release、优化开关和异常断言保持原样。独立测试检查容量、淘汰、
重新实例化、输入验证及已返回实例的所有权。无需增加 Node 堆限制或跳过 case。
