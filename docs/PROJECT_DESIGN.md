# PROJECT_DESIGN — APM 监控 UI

2026-09-24 确认范围：只打磨现有单页操作台的层级和扫描性。身份保留：浅底、蓝色主操作、工程密度。不重置风格，不改查询逻辑、文案、按钮四态、场景回写、combobox、复制纯文本，不加依赖。

## 1. Product Context

- Product: Windows 桌面工具，把腾讯云 APM、TKE、MySQL / Redis / MongoDB 收成一段可贴群的统计。
- Target user: 自己跑统计、复制结果、盯日志的运维使用者。
- Target surface: `ui/index.html` 这一页。顶栏、设置、场景 / 监控对象 / 时间、应用列表 + 指标列表、开始统计、结果、实时日志。
- Primary job-to-be-done: 选好对象和指标，开始统计，扫结果数字，复制。
- Success criteria: 主操作、当前选择、异常数字比现在更容易一眼分开；页面仍是紧凑工具，不是展示页。
- Content/data that must appear: 现有全部控件与结果行。不增区块，不造指标。
- Interaction requirements: 两个「开始统计」仍同步 loading；状态色仍是中性 / 忙蓝 / 成功绿 / 失败红；结果语义色规则不变（错误类 > 0 红，错误率 = 0 绿）。结果按数据源异步输出：每个数据源一返回就先渲染，不等全部完成，顶部提示「正在统计…（n/m）」。看日志时结果插入不得把日志顶出视口：`keepViewStable` 在日志面板仍与视口相交时，按它的屏幕坐标补偿滚动。结果区关闭浏览器滚动锚定（`overflow-anchor: none`），避免清空节点时失锚。
- Technical constraints: 原生 HTML/CSS/JS。改动以 `ui/style.css` 为主。允许去掉两处只为样子服务的内联样式。

## 2. Existing UI Read

- Current visual vocabulary: 白卡片、12px 圆角、蓝渐变主按钮、渐变选中 chip、页面径向蓝光、结果块斑马纹、深色日志。
- Strongest existing cue to preserve: 浅底 + 单一蓝色主操作 + 13px 工程密度。
- Components/tokens to reuse: `.card` `.chip` `.tab` `.btn-primary` `.btn-ghost` `.btn-mini` `.check-list` `.result-block` `#log-box` 和现有 CSS 变量名。
- Patterns to preserve: 三段式任务流（配置一行、选择双列、结果在日志之上）；列表高度；指标分组 sticky；日志深色终端。
- Patterns to evolve: 选中态、卡片标题、顶栏、结果数字、主按钮。让「当前选择」和「异常值」重于装饰。
- Patterns to remove or avoid: 页面径向光斑、按钮/chip 的蓝渐变和发光阴影、底部「开始统计」比顶栏更大。
- Accessibility/state conventions already present: focus ring、disabled、hover、`.ok` `.err` `.busy`、loading spinner。全部保留。

## 3. Taste Direction

- Product identity sentence: 一台浅色、冷静、用来统计和复制的精密操作台。
- Recommended taste direction: 浅色分析工作台。一层灰底、白卡片、一根蓝线标出当前选择和主操作。选中是淡蓝底，不是发光胶囊。
- Direction to avoid: 深色驾驶舱、大标题、装饰渐变、把监控台做成品牌展示页。
- Why this makes the UI more useful: 现在卡片、chip、按钮、标题重量接近，扫结果时眼睛要自己找主次。收掉发光之后，蓝色只出现在「正在用」和「下一步」。
- What should feel distinctive: 结果区的数值、选中行的淡蓝、顶栏那一颗实心主按钮。
- What should stay quiet: 分区标题、未选中 chip、边框、辅助说明。

## 4. Selected References

支撑材料，不是要抄的界面。

### Sentry（监控密度）

- Why it fits: 监控工具里，状态和数值要比装饰先被看见。
- Transferable traits: 字重分层（标签轻、数值重）；等宽数字；状态色克制地只用在语义上。
- Non-transferable brand details: 深紫底、柠檬绿、粉色焦点、毛玻璃、大写按钮、展示字体。
- Implementation substitutions: 继续用现有蓝 `#2563eb` 和浅底。日志保持唯一的深色表面。
- Risk: 把「监控感」做成暗色主题。
- How to weaken: 只取层级，不取色彩和材质。

### Airtable（浅色结构化列表）

- Why it fits: 白底、海军蓝字、一根蓝作为交互色，列表行可以扫。
- Transferable traits: 白表面、细边框、弱次级文字、选中用浅底而不是重填充。
- Non-transferable brand details: Haas 字体、16–32px 大圆角、多层蓝光阴影、18px 正文、彩色标签。
- Implementation substitutions: 字体仍是 Segoe UI / 微软雅黑。圆角停在 8–10px。阴影几乎去掉。
- Risk: 列表变得松、圆、像协作产品。
- How to weaken: 密度维持 13px，圆角不加大。

### IBM Carbon（蓝色系统，只取结构）

- Why it fits: 一个强调色、用表面分层代替阴影、紧凑 UI 的小字距。
- Transferable traits: 页面灰、卡片白、列表井再浅一档；选中行用淡蓝底；12px 说明文字略加字距。
- Non-transferable brand details: 0 圆角、只有底边的输入框、48px 高按钮、IBM Blue `#0f62fe`、300 字重大标题。
- Implementation substitutions: 保留 8–10px 圆角和盒状输入框。蓝色继续 `#2563eb` / `#1d4ed8`。
- Risk: 界面变成直角企业规范，丢掉现在已经认得出的卡片工具感。
- How to weaken: 圆角和输入框形态不动。

## 5. Visual Theme & Atmosphere

- Design thesis: 灰底上的白卡片。蓝色是电流，只走主按钮、当前 Tab、焦点和选中。
- Emotional tone: 冷静、可控、精确。
- Product personality: 值夜班用的统计台，不是产品首页。
- First viewport message: 顶栏品牌 + 状态 + 一颗实心「开始统计」。下面是三张平的配置卡。
- Visual weight priorities: 1) 主按钮与状态 2) 选中项与结果异常数字 3) 列表正文 4) 分区标题和说明。

## 6. Color Palette & Roles

沿用变量名，改值：

- Page/background: `--bg: #f3f5f8`。去掉径向渐变。
- Primary surface: `--card: #ffffff`
- Secondary/elevated surface: `--well: #f6f8fb`（列表、结果头、输入底）
- Selected surface: `--selected: #e7f0fe`
- Primary text: `--text: #1a2332`
- Secondary text: `#3d4a5c`（结果标签）
- Muted text: `--muted: #6d788a`
- Accent/CTA: `--primary: #2563eb`，按下/深色 `--primary-deep: #1d4ed8`
- Border/divider: `--border: #e1e6ee`
- Focus ring: `0 0 0 3px rgba(37, 99, 235, 0.16)`
- Success/warning/error: `--ok: #15803d`，`--danger: #dc2626`。不新增警告色。
- Color constraints: 渐变只允许品牌小方点那一处。chip、按钮、页面背景不再用渐变。

## 7. Typography Rules

- Font families and fallbacks: 界面 `"Segoe UI", "Microsoft YaHei", system-ui, sans-serif`。数字与日志 `Consolas, "Cascadia Mono", monospace`。
- Display/hero: 无。品牌名 16px / 650。
- Section headings: `.card-title` 12px / 600 / `--muted`，字距 0.04em。不再用 13.5px 粗黑标题和内容抢。
- Subheadings: Tab 13px；激活 650，颜色 `--primary-deep`。
- Body: 13px / 400 / 行高 1.45。
- Labels/captions: 12px。字段标签用 `--muted`。
- Code/mono: 结果内容 12.5px。数值 `font-variant-numeric: tabular-nums`。
- Weight rules: 400 正文，600 标签和按钮，650 品牌与结果标题。避免满屏 700。
- Line-height rules: 控件紧凑；日志 1.6 保持。
- Letter-spacing rules: 只给分区标题和指标分组 0.04em。正文不拉字距。

## 8. Component Styling

### 顶栏

- Background: 粘在顶部时用 `--bg` 实底，底边 `1px solid var(--border)`。去掉向下透明的渐变。
- 主按钮维持右侧。状态文字保持现有语义 class。

### 卡片

- Background: `#fff`
- Border: `1px solid var(--border)`
- Radius: 10px
- Shadow: 无，或仅 `0 1px 0 rgba(26, 35, 50, 0.04)`。下拉列表保留真正的浮层阴影。
- Padding: 12px 14px 不变。

### 主按钮

- Background: 实心 `--primary`，无渐变。
- Text: 白，600。
- Radius: 8px。`.btn-big` 保持约 15px、左右 28px。
- Shadow: `0 1px 2px rgba(37, 99, 235, 0.28)`。
- Hover: 背景 `--primary-deep`，不抬起、不加亮。
- Active: 背景再深一档 `#1e40af`。
- Disabled / loading: 现有透明度与 spinner 不动。
- 底部 `#btn-query-2` 与顶栏同一尺寸。去掉内联的 `padding: 11px 64px; font-size: 16px`。

### 次按钮

- `.btn-ghost`：白底、细边框、蓝字。Hover 用 `--selected`。
- `.btn-mini`：白底细边框。`.active` 用实心蓝，这是「全部/全选」当前筛选，可以实心。
- Hover / focus 规则与现在一致，只是去掉多余发光。

### Chips（场景、数据源、时间）

- 默认：白底、细边框、12.5px、圆角 999px 可保留（这是筛选胶囊，不是卡片）。
- Hover: 边框变蓝，字变蓝，背景不变。
- Active: 背景 `--selected`，字 `--primary-deep`，边框 `--primary`。无渐变、无阴影。
- 多选时多个淡蓝胶囊比一排发光蓝块更容易数。

### Tabs

- 底边 1px `--border`。
- 默认透明、`--muted`。
- Hover: 字变蓝，背景透明。
- Active: 字 `--primary-deep`、600，底边 2px `--primary`。去掉激活态的浅蓝底块。
- Badge: 默认灰底；激活 Tab 的 badge 用 `--selected` 和蓝字，不再变成实心蓝豆。

### 列表

- `.check-list`：背景 `--well`，边框、10px 圆角、320px 高度不变。
- 行 hover: `#eef2f7`。
- 已勾选行: `.check-item:has(input:checked)` 背景 `--selected`。
- 指标分组头：11px / 600 / `--muted`，背景与列表井相同，字距 0.04em。取消全大写。

### 表单

- 输入框背景 `--well`，聚焦变白，边框蓝 + 现有焦点环。
- 设置里的虚线认证框改为实线细边框，背景 `--well`。

### 结果

- 块：白底、细边框、8px 圆角、块间距 8px。
- 头：背景 `--well`，标题用 `--text` 而不是蓝色，避免每条服务名都像链接。
- 行：标签与数值连排（`标签：值`），标签 `#3d4a5c`，数值 600、tabular-nums。斑马纹用 `#f8fafc`。
- `.v.bad` / `.v.good` 颜色规则不变。
- 空状态：左对齐，内边距与结果行一致。去掉居中和 24px 大留白。

### 日志

- 保持 `#0f172a` 底、浅字、错误粉、成功绿。这是唯一深色表面。
- 面板卡片本身仍是浅色。

### 窄屏

- 现有 1150px 配置区折行保留。
- 双列在约 860px 以下改为上下堆叠，列表 `min-width` 不再把页面撑出横向滚动。

## 9. Layout Principles

- Spacing scale: 4 / 6 / 8 / 10 / 12 / 14。不引入新的大段空白。
- Container width: 维持现在的流式全宽，不居中成阅读栏。
- Grid: `#config-row` 三卡一行；`#main-cols` 列表约 55/45。
- Section rhythm: 卡片间距 10px。
- Density model: 生产型紧凑。标题变轻，行距不加大。
- Whitespace philosophy: 空白只用来分开三段任务，不用来「透气」。
- Breakpoints: 1150px 配置折行；860px 双列堆叠。
- Mobile collapse strategy: 这是桌面 WebView。只保证窗口变窄时不横向溢出。

## 10. Depth, Motion, And Interaction

- Elevation levels: 0 页面与卡片；1 仅 combobox 下拉（`0 8px 24px rgba(26, 35, 50, 0.12)`）。
- Border/ring/shadow rules: 分隔靠 1px 边框和表面色差。
- Motion personality: 颜色与边框 120–150ms。按钮 hover 不再 `translateY`。
- Transition rules: 保留 spinner。不新增入场动画。
- Touch target rules: 桌面工具。现有点击区域不缩小。

## 11. Do's And Don'ts

### Do

- 蓝色只留给主操作、当前 Tab、焦点、选中和忙状态。
- 用表面色差区分页面、卡片、列表井。
- 让结果数值等宽对齐，异常色保持现在的规则。
- 日志继续用深色终端。

### Don't

- 不要深色主题、径向光、玻璃、大标题、新区块。
- 不要把圆角收到 0，也不要放大到 16px 以上。
- 不要改文案、DOM 行为约定和 `app.js` 的数据逻辑。
- 不要换成参考品牌的色值或字体。

## 12. Implementation Mapping

- Files likely to change: `ui/style.css`。`ui/index.html` 只删 `#btn-query-2` 的放大内联样式，以及空状态上的居中内联样式。
- Existing components to reuse: 全部现有 class。
- Tokens/classes/variables to extend: `:root` 增加 `--well` `--selected`。
- New components needed: 无。
- Assets needed: 无。
- Data/copy assumptions: 预览用现有 `_preview.html`，不改真实数据。

## 13. Evaluation Plan

- Build/typecheck: 无构建。CSS 能被页面加载即可。
- Browser/screenshot: 打开 `ui/_preview.html`，看顶栏、配置三卡、双列列表、结果、日志。
- Responsive: 宽窗三卡一行、双列并排；窄于 860px 时双列堆叠且无横向滚动。
- Contrast/readability: 主文字、淡蓝选中上的蓝字、日志浅字。
- Interaction states: chip 选中、Tab 激活、行勾选、主按钮 hover、空结果。
- Product fit: 仍像原来的 APM 监控，只是更好扫。
- Reference alignment: 借了浅底分层和监控层级，没有变成 Sentry 暗色或 Airtable 圆角产品站。
- Generic UI regression: 没有新的渐变英雄区、没有假指标。
- Better-than-original check: 选中项和结果数字比发光蓝块更容易数。
