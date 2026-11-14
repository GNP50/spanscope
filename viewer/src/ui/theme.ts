import { useMemo } from 'react';
import * as echarts from 'echarts/core';
import { BarChart, LineChart, ScatterChart } from 'echarts/charts';
import { GridComponent, LegendComponent, MarkLineComponent, TooltipComponent } from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import { useProfile } from '../store';

echarts.use([BarChart, LineChart, ScatterChart, GridComponent, LegendComponent, MarkLineComponent, TooltipComponent, CanvasRenderer]);
export { echarts };

/** Resolved theme colors for canvas, ECharts, and WebGL, which cannot read `var(--x)`. */
export interface ThemeColors {
  text: string;
  muted: string;
  line: string;
  surface: string;
  accent: string;
  highlight: string;
  warning: string;
  danger: string;
  info: string;
  dim: string;
  /** Text drawn on palette fills. */
  ink: string;
  palette: string[];
}

function read(name: string, fallback: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
}

export function readThemeColors(): ThemeColors {
  return {
    text: read('--text', '#e6f0ed'),
    muted: read('--muted', '#8aa1a2'),
    line: read('--line', '#26383b'),
    surface: read('--surface', '#101c20'),
    accent: read('--accent', '#72e1c7'),
    highlight: read('--highlight', '#f1b871'),
    warning: read('--warning', '#efba77'),
    danger: read('--danger', '#ef8e7f'),
    info: read('--info', '#80b6f2'),
    dim: read('--dim', '#2a3a3d'),
    ink: read('--ink', '#0b1a1a'),
    palette: Array.from({ length: 8 }, (_, i) => read(`--palette-${i}`, '#72e1c7')),
  };
}

/** Theme colors, recomputed when the theme switches. */
export function useThemeColors(): ThemeColors {
  const theme = useProfile(state => state.theme);
  // The data-theme attribute is set synchronously by the store subscriber.
  return useMemo(() => readThemeColors(), [theme]);
}
