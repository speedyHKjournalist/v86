# PR #51 最终本地核验（2026-09-21）

基线：`e008f74c437e2ae1c3b063ecd4e20b3a015c75f3`。
最终执行代码：`8cb021dae7bea4de05c4f93c2731a6f62777349e`，源码树
`a923f67f0cb285102e976be639e505ae884337ab`；本文档提交不改变执行代码。
环境：Linux x86-64、Rust 1.98.1、Node 24.17.0。
**未取得 XP 镜像，未进行 XP 启动复测，也未证明性能已追平 legacy。**

[实施与首次测量](ir-admission-publication-20260921.md) 属于首个候选提交
`53a7b941`。下面单独记录后续修正和最终复测，不能混用不同候选的指纹或样本。

## 后续修正

保留空扫描后的编译额度会带来新的风险：已编译入口也可能反复进行当前入口查找。
增加 `interpreted_probe`：完整热度环扫描仍每帧最多一次；只有出现新的解释执行，
才允许额外尝试当前入口。缓存中的 Tier 2 执行不会为此重新查询热度索引。
这只是调度提示，不授权执行、不取代发布身份、源码、映射和异常检查。

无 SIMD 回归还持有启动时创建的 Uint32Array；在线编译导致 Wasm memory.grow 后，
该视图已经失效。测试改为从当前 memory.buffer 读取计数，保留精确退休数、SIMD
解释回退、失败编译抑制和零 legacy 编译的全部断言。

初始提供的 JS 包缺少当前 `compiler_breakdown` 诊断字段。已从本 PR 源码重新构建
JS，诊断断言没有删除；最终配对的全部方案使用同一份匹配源码的 JS。

## 本地正确性验证

- 最后一次原生测试：250 项通过，2 项显式微基准忽略。
- 最终调度修改后，debug/release/正式核心的提交交还、自动发布、融合、共享入口、
  缓存生命周期回归通过；诊断计数与时间守恒通过。
- 本轮入口/live、STI、helper reload、SSE FP、MMX、SIMD move/integer、CMPXCHG8B、
  store continuation、helper audit 及 MIR/Wasm 执行回归通过。
- x87 显式精确模式下，debug/release 各 99,840 个特殊值案例及原有寄存器、FLAGS、
  CR0 优先级矩阵通过；qNaN/sNaN payload 和负数域有额外独立断言。
- 无 SIMD loader/Worker 路径参数、标量 IR 与向量解释回退通过。
- 补齐 NDISASM 后，16,384 个独立解码边界对照通过。
- production/ir-experimental 的 warning-free Wasm 检查通过。

以上不是完整远端 CI 或实际浏览器/Worker 全矩阵通过的声明；远端结果以 PR 对应
提交的 Actions 为准。打开 IR 诊断时暖准入保留保守路径，评估新暖路径应使用关闭
诊断的计时，或另跑关闭诊断的 V8 CPU sampling，不能直接用诊断模式耗时外推。

## 最终固定工作量结果

无诊断、独立 VM、串行交替，无并发构建/回归；退休数和最终架构状态严格检查。

### 普通跨页短区域

4,000,000 次算术更新，每片段 3 条算术指令，四页循环，关闭融合以隔离普通边界。
各方案三轮：

| 中位数 | 基线 IR | 本次 IR | 本次 legacy |
| --- | ---: | ---: | ---: |
| 毫秒 | 429.343 | 341.047 | 118.555 |

本次 IR 吞吐比基线提高 **25.89%**。三轮本次/基线为
384.111/429.343、341.047/423.773、336.729/535.812 ms。
**本次仍只有 legacy 吞吐的 34.76%**；这证明有局部改善，也证明剩余边界成本很大。
原始结果：`accepted-page3.jsonl`。

### 冷编译与发布

32 个分离页面，严格完成 12,582,978 条客户机指令；各方案三轮：

| 中位数 | 基线 IR | 本次 IR | 基线 legacy | 本次 legacy |
| --- | ---: | ---: | ---: | ---: |
| 毫秒 | 436.094 | 419.911 | 242.272 | 241.786 |
| mIPS | 28.854 | 29.966 | 51.937 | 52.042 |
| IR 退休覆盖 | 13.58% | 19.46% | 0 | 0 |

IR 中位吞吐提高 **3.85%**。三轮本次/基线为
412.798/449.995、419.911/435.619、438.129/436.094 ms；**第三轮仍变慢**。
不能把早先候选的一次 +21.45% 冷态结果当作本版稳定收益。
原始结果：`accepted-cold.jsonl`。

### 原有固定工作量门槛，七轮

| 工作负载 | 本次 IR / legacy 吞吐 |
| --- | ---: |
| 整数 | 0.928 |
| RAM RMW | 0.999 |
| 间接区域 | 1.745 |
| SSE 寄存器 | 0.981 |
| 几何平均 | **1.123** |

通过原门槛：几何平均至少 1.0，且每项至少 0.9。没有修改门槛。
原始结果：`accepted-fixed.jsonl`。

保留失败记录：首个候选匹配 JS 的三轮复测中，RAM RMW 为 0.890882，导致单项
门槛失败（`matching-fixed.jsonl`）。五轮 RAM 专项的基线/候选比值分别为
0.949109/0.932819；它未复现持续的 0.89 水平，但不能证明不存在波动或小幅回退。
最终在限制重复当前入口探测后，按预定七轮重跑完整固定工作量；上述最终结果通过。
失败样本没有删掉，也不以通过的几何平均掩盖先前单项失败。

## 最终指纹与复现

- 基线 Wasm：`180c3b8773ead21a6d260c05798d301eef7ab3bbbe856aade813ae2c67df34a6`
- 最终 Wasm：`b6cdd510dd5d86770963edb3073665bbd3d1400cdd165487ed8edd283604944e`
- 匹配源码 JS：`3e597e24600dff3e11305835bb6e9648b9273c1a30157961b6f190a25b563947`

```sh
IR_COMPARE_RUNS=7 node tests/ir/performance/fixed_work.mjs
IR_BOUNDARY_UPDATES=4000000 node tests/ir/performance/boundaries.mjs build/v86-ir-runtime.wasm /path/to/e008f74c.wasm
node tests/ir/performance/cold_publication.mjs build/v86-ir-runtime.wasm /path/to/e008f74c.wasm
IR_BASELINE_WASM=/path/to/e008f74c.wasm node tests/ir/performance/xp_compare.mjs /path/to/windowsxp.img build/ir-admission-xp
```

边界命令运行完整六种形态；最终 page3 专项从相同程序中只选择该 workload，
保持预热、顺序、计时和全部架构断言不变。XP 同镜像配对仍是未完成的系统验收。
