# IR-00～IR-12 后续增量（2026-09-20）

本次实现两个具体缺口，不把 IR-13 性能门槛混入早期包的功能完成声明。

## IR-00：可执行的公开优化配置

增加 `ir_opt_level: 0|1|2` 和 `ir_passes_disabled`，贯通 V86 构造、CPU Worker、
自动 Tier-1/Tier-2 编译、`get_jit_info()` 和 `index.html` 的启动参数保留。
低层配置入口仅允许未启用调度、无缓存产物和无待发布任务的冷初始化状态；
reset/restore 保留目标实例的编译策略，不从客户机快照导入编译器配置。

17 个 pass 名称的语义见 [公开后端说明](ir-backend.md)。关闭优化仍必须解码、
lower、分配必要 locals、验证 proof/StateMap、检查访存并维护精确异常。
关闭 `ram_forward` 时，`ram_loop` 不再隐式建立普通 forwarding 证书。

原生测试比较“全部禁用”与无优化模块、比较受限 Tier-2 与 Tier-1 模块；
共享 Node/浏览器主线程/Worker 场景通过实际编译计时计数确认禁用阶段未调用，
其他阶段仍调用，并执行升档、快照恢复、重启、非法配置及复制结果的检查。

## IR-06/IR-11：RMW 提交后的值复用

owned MIR 从仿射 RMW ticket 的定义追溯原始写权限检查及地址。只有地址、
段和宽度完全匹配的后续普通读取可使用新写入值；段检查保留原位置。
新值仅在 native RAM 写入成功、代码依赖检查通过后写入独立缓存 local。
冷路径、MMIO、跨页慢路径、代码物理别名仍提交后退出，不建立续执行证明。
中间可能观察设备或修改映射的动作会截断证明；首次写权限故障不提交指令。

证明不依赖 HIR 生命周期，发射前独立重算。新增伪造证书拒绝、不同宽度、
不同段/地址和中间访存负例。CPU 差分覆盖 8/16/32 位、优化开关、线性/CFG、
冷暖 RAM、跨页、代码别名、只读页、第二页缺页和 MMIO 回调修改代码。

## 当时尚未收口

后续进展见 [公开诊断与 helper 扩展](ir-debug-helper-followup.md)；以下为本次增量当时的边界。

- IR-00：公开 `ir_verify`、`ir_dump`、命名统计模式仍未实现；已有采样诊断 API。
- IR-02/05～08：现有粗粒度目录和差分矩阵不能等同于全部模式、前缀、故障组合
  的完整验收；基线未实现的 ISA/任务切换分支没有被新增为支持。
- IR-03/04：任意可变 MIR 图的独立 verifier、更一般的观察点状态同步仍有缺口。
- IR-12 扩展：含观察 helper 的三/四源区域仍拒绝发布；此前 XP 暴露的隐藏状态
  问题尚未定位。本次未放开此限制。
- IR-13/14：XP 性能和默认切换仍未完成。本次功能验证不声称达到 legacy 性能。

运行入口：`cargo test --lib`、`make ir-backend-integration-tests
ir-backend-browser-tests`、`node tests/ir/differential/store_continuation.mjs`。
后一个命令依赖原生测试生成的 fixture 和已构建的 IR 测试核心。

## 本地验证

- 231 项原生测试全部通过（`build/ir-gap-final-native.log`）。
- 新增 104 组 RMW 对照在 debug/release 核心均通过；原有 forwarding 的 RAM、
  #PF/#GP、MMIO 重映射、预算和 CPL 矩阵继续通过。
- Node、Chromium 主线程和真实 Worker 的优化配置执行与拒绝矩阵通过。
- 无 SIMD 便携核心的公开配置、升档、恢复和错误处理通过。
- 生产核心的缓存、自动升档、SMC、264 入口淘汰及诊断守恒测试通过。

日志分别为 `build/ir-gap-rmw-{debug,release}.log`、
`build/ir-gap-forwarding.log`、`build/ir-gap-final-{backend,browser,auto}.log`、
`build/ir-gap-portable.log`、`build/ir-gap-cache.log` 和
`build/ir-gap-diagnostics.log`。

关闭诊断、无并发构建/测试的三轮固定工作量复测（相同指令数及最终状态）：

| 工作量 | IR 中位 mIPS | Legacy 中位 mIPS | 比值 |
| --- | ---: | ---: | ---: |
| 整数 | 182.052 | 184.214 | 0.988 |
| RAM RMW | 108.514 | 114.826 | 0.945 |
| 间接区域 | 108.859 | 67.886 | 1.604 |
| SSE 寄存器 | 50.759 | 53.372 | 0.951 |

几何平均 **1.092 倍**，通过既有固定工作量门槛；部分单项仍低于 legacy。
原始结果：`build/ir-gap-fixed-work.jsonl`。这不是 XP 冷启动性能验收结果。

同一 XP 镜像的单次 IR 冷启动冒烟到达首次 800×600×32 模式（34.701 秒，
`build/ir-gap-xp-smoke.jsonl`），使用内存写覆盖层。本次没有重跑 XP 三组配对
性能验收，不能用这一次结果宣称 XP 提速或完整桌面就绪。
