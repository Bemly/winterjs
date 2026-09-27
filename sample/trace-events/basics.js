// node:trace_events: create/enable/disable a tracing category.
// trace_events：创建、开关追踪分类。
// Run / 运行: winterjs --run sample/trace-events/basics.js
import { createTracing, getEnabledCategories } from 'node:trace_events';

const tracing = createTracing({ categories: ['node'] });
console.log('[trace] enabled before:', tracing.enabled === false);
tracing.enable();
console.log('[trace] enabled after:', tracing.enabled === true);
console.log('[trace] categories type:', typeof getEnabledCategories() === 'string');
tracing.disable();
console.log('[trace] disabled:', tracing.enabled === false);
