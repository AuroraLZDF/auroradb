/**
 * ECharts 单一动态入口（V3-4.2 包体积回归）：
 * 按需注册用到的图表类型与组件，ChartRenderer 通过 `import('@/utils/echarts')`
 * 懒加载本模块——只产生一个独立 chunk，避免多入口动态导入导致共享模块重复打包。
 */
import * as echarts from 'echarts/core'
import { BarChart, LineChart, PieChart, ScatterChart } from 'echarts/charts'
import { GridComponent, TooltipComponent, LegendComponent } from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'

echarts.use([BarChart, LineChart, PieChart, ScatterChart, GridComponent, TooltipComponent, LegendComponent, CanvasRenderer])

export default echarts
