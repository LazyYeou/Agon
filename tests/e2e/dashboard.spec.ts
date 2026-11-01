import { test, expect } from '@playwright/test';
test('replay tower, telemetry, charts and saved layout work through real gateway',async({page})=>{
  await page.goto('/');await expect(page.getByRole('heading',{name:'Agon',exact:true})).toBeVisible();
  await expect(page.getByText('Synthetic demo', {exact:true})).toBeVisible();
  await expect(page.getByRole('button',{name:'Open telemetry for VER'})).toBeVisible();
  await page.getByRole('button',{name:'Play replay'}).click();
  await expect(page.getByRole('button',{name:'Pause replay'})).toBeVisible();
  await page.getByRole('button',{name:'Pause replay'}).click();
  await page.getByRole('button',{name:'Open telemetry for VER'}).click();
  await expect(page.getByRole('dialog',{name:'VER telemetry'})).toBeVisible();
  await expect(page.getByText('Approximate telemetry', {exact:true})).toBeVisible();
  await page.getByRole('button',{name:'Expert'}).click();await expect(page.getByRole('heading',{name:'Race history'})).toBeVisible();
  await page.getByRole('button',{name:'Close telemetry'}).click();
  await page.getByRole('button',{name:'Charts',exact:true}).click();await expect(page.getByRole('heading',{name:'Driver comparison'})).toBeVisible();
  await page.getByLabel('Chart type').selectOption('degradation');
  await page.getByRole('button',{name:'Pin chart'}).click();
  await page.getByRole('button',{name:'Tower',exact:true}).click();await expect(page.getByText('Pinned · Tyre degradation')).toBeVisible();
  await page.getByRole('button',{name:'Columns',exact:true}).click();
  await page.getByLabel('Show Pit stops').uncheck();
  await page.getByRole('button',{name:'Done',exact:true}).click();await page.reload();
  await page.getByRole('button',{name:'Columns',exact:true}).click();await expect(page.getByLabel('Show Pit stops')).not.toBeChecked();
});
test('mobile navigation and keyboard close work',async({page})=>{
  await page.setViewportSize({width:390,height:844});await page.goto('/');
  await page.getByRole('button',{name:'Map',exact:true}).click();await expect(page.getByRole('heading',{name:'Track map'})).toBeVisible();
  await page.getByRole('button',{name:'Tower',exact:true}).click();await page.getByRole('button',{name:'Open telemetry for NOR'}).click();
  await page.keyboard.press('Escape');await expect(page.getByRole('dialog')).toHaveCount(0);
});
