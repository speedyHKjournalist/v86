# IR-11：有界纯值 LICM

本次是 IR-11 的部分实现，不是 IR-00～IR-14 完成声明。默认 CPU 后端、
生产覆盖门禁、解释器回退与 legacy emitter 均保持不变。

## 实现

`analysis::loops` 根据共享 CFG 的支配关系发现自然循环，合并同一 header 的
多个 latch，按内层优先处理。只有唯一且无条件跳向 header 的外部前驱可作为
preheader；不拆分边，不创建入口，不改变参数。循环包含外部执行入口、不可约
反向闭包或没有合格 preheader 时不外提。

`passes::licm` 只接受显式白名单中的全定义纯 SSA 运算：整数算术、位操作、
选择、类型转换、位计数、已解析地址的纯偏移和纯 packed-integer/vector 运算。
`!op.ordered()` 不是可投机执行证明。CPU 状态读取、访存、地址/权限/SSE 守卫、
除法、helper、预算检查及带 StateMap/commit 的节点均不移动。

操作数必须在循环外定义且支配 preheader。新位置为 preheader 的末尾，已有
观察点不变。固定点扫描支持非支配顺序的 block 分配；已外提的定义可为后续
外提提供输入。嵌套循环可以把同一节点分步移向更外层。

变换先在候选 Region 上执行，最终 verifier 成功后整体提交。工作预算耗尽或
验证失败不改变调用方的 Region，包括已规划的部分移动。无合格循环时避免
复制 HIR arena。CFG、StateMap、effect 链、寄存器布局及客户机指令计数均不改写。

## 配置与限制

`PassConfig::licm` 是独立开关；默认启用，仅在调用方原本选择优化时执行。
调用位置为每轮 GVN 之后、DCE 之前。现有三个编译 API 共用此优化管线。
`PassStats::loops` 记录各轮可处理循环的累计次数，`hoisted` 记录移动次数；
两者都不代表去重后的循环或指令数量。

独立调用 `licm::run(&mut region, work_budget)` 可设置工作上限。默认上限为
1,048,576；大于 16,777,216 的请求拒绝。区域硬限制为 64 blocks、8,192
instruction slots、16,384 value slots、8,192 StateMaps 和 8,192 helpers。
循环扫描及操作数检查计入 work；CFG/verifier 由 arena 上限约束，不把 work
计数描述为包含所有内部操作的时钟预算。

## 验证入口与已执行结果

```sh
make ir-licm-tests
# 完整已有矩阵仍通过原有入口执行：
make ir-tests
```

2026-09-13，在下载的精确源快照上使用 rustc 1.98.1、Node.js 22.16.0 执行：

- `RUSTFLAGS="-D warnings" cargo test`：135 项通过，包括 8 项新增 LICM 测试。
- `node tests/ir/wasm/run.mjs`：已有 Wasm 执行矩阵及新增 LICM oracle 通过。
- 新增 oracle 实际执行 11,520 次 Wasm：优化前后比较全部 12 个标量状态槽，
  覆盖零次循环、9 种退出预算、i32/i64 溢出、反向 block 分配和恢复专用值；
  正常完成另用独立 BigInt 计算结果，不仅进行同源差分。
- Rust 结构/负向测试检查多个 latch、嵌套循环、多入口/不可约 CFG、独立关闭、
  预算失败原子性、访存/helper/观察点保留、纯 SIMD 数据流与非法入口读取拒绝。

SIMD 此处包含结构验证，不宣称完成 SIMD 指令覆盖或取得游戏性能收益。
GitHub Actions 增加 LICM 专项入口和编译/部分语义/缓存/Tier/后端回归入口；
实际 CI 成功与否以对应提交的运行结果为准，不用本地结果替代远端结果。

## 仍未完成

proof-based 访存复用、load/store forwarding、访存 LICM、循环归纳变量优化、
通用 SIMD 优化、其余 ISA、完整共享版本/链接管理、Windows XP/应用性能验收、
默认后端切换与 legacy 退役均不在本次完成范围内。生产 Pending 仍为 3,728，
`ir-default-gate` 必须继续拒绝把当前状态当作完整生产覆盖。
