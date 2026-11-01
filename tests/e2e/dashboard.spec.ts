import { test, expect } from "@playwright/test";
test("replay tower, telemetry, charts and saved layout work through real gateway", async ({
  page,
}) => {
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "Agon", exact: true }),
  ).toBeVisible();
  await expect(page.getByText("Synthetic demo", { exact: true })).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Open telemetry for VER" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Play replay" }).click();
  await expect(
    page.getByRole("button", { name: "Pause replay" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Pause replay" }).click();
  await page.getByRole("button", { name: "Open telemetry for VER" }).click();
  await expect(
    page.getByRole("dialog", { name: "VER telemetry" }),
  ).toBeVisible();
  await expect(
    page.getByText("Approximate telemetry", { exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Expert" }).click();
  await expect(
    page.getByRole("heading", { name: "Race history" }),
  ).toBeVisible();
  await page.getByRole("combobox", { name: "Reference lap", exact: true }).selectOption("1");
  const start = await page.locator(".timeline input").getAttribute("min");
  await page.locator(".timeline input").evaluate((el, value) => {
    const input = el as HTMLInputElement;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  }, start!);
  await expect(page.getByText("No telemetry samples for this lap. Select another completed lap.")).toBeVisible();
  await page.getByRole("button", { name: "Close telemetry" }).click();
  await page.getByRole("button", { name: "Charts", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Driver comparison" }),
  ).toBeVisible();
  await page.getByLabel("Chart type").selectOption("degradation");
  await page.getByRole("button", { name: "Pin chart" }).click();
  await page.getByRole("button", { name: "Tower", exact: true }).click();
  await expect(page.getByText("Pinned · Tyre degradation")).toBeVisible();
  await page.getByRole("button", { name: "Columns", exact: true }).click();
  await page.getByLabel("Show Pit stops").uncheck();
  await page.getByRole("button", { name: "Done", exact: true }).click();
  await page.reload();
  await page.getByRole("button", { name: "Columns", exact: true }).click();
  await expect(page.getByLabel("Show Pit stops")).not.toBeChecked();
});
test("mobile navigation and keyboard close work", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await page.getByRole("button", { name: "Map", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Track map" })).toBeVisible();
  await page.getByRole("button", { name: "Tower", exact: true }).click();
  await page.getByRole("button", { name: "Open telemetry for NOR" }).click();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("phone swipe navigation and arrow keys reach the three timing views", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  const workspace = page.locator(".workspace");
  await expect(page.getByRole("button", { name: "Open telemetry for VER" })).toBeVisible();
  await workspace.evaluate(el => {
    el.dispatchEvent(new TouchEvent("touchstart", { bubbles: true, touches: [new Touch({ identifier: 1, target: el, clientX: 320, clientY: 300 })] }));
    el.dispatchEvent(new TouchEvent("touchend", { bubbles: true, changedTouches: [new Touch({ identifier: 1, target: el, clientX: 70, clientY: 310 })] }));
  });
  await expect(page.getByRole("button", { name: "Map", exact: true })).toHaveAttribute("aria-pressed", "true");
  await page.getByRole("button", { name: "Map", exact: true }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(page.getByRole("heading", { name: "Driver comparison" })).toBeVisible();
  await workspace.evaluate(el => {
    el.dispatchEvent(new TouchEvent("touchstart", { bubbles: true, touches: [new Touch({ identifier: 1, target: el, clientX: 70, clientY: 300 })] }));
    el.dispatchEvent(new TouchEvent("touchend", { bubbles: true, changedTouches: [new Touch({ identifier: 1, target: el, clientX: 320, clientY: 310 })] }));
  });
  await expect(page.getByRole("button", { name: "Map", exact: true })).toHaveAttribute("aria-pressed", "true");
});

test("accuracy shows eligible-stop coverage and keeps original forecasts when pinned", async ({ page }) => {
  await page.route("**/api/accuracy", route => route.fulfill({ json: {
    status: "measured", model: "test", pace_samples: 6769, pace_mae: 1.218,
    pace_1_to_5_mae: 1.526, pit_samples: 219, pit_candidates: 461,
    pit_accuracy: .2511, pit_forecast_coverage: .4751,
  }}));
  await page.goto("/");
  await page.getByRole("button", { name: "Charts", exact: true }).click();
  await page.getByLabel("Chart type").selectOption("accuracy");
  await expect(page.getByText(/219 scored \/ 461 eligible stops; forecast coverage 47.5%/)).toBeVisible();
  await expect(page.getByText(/Next 1–5 laps MAE: 1.526 s/)).toBeVisible();
  // Initial replay issues real gateway forecasts before the next lap completes.
  const chart = page.locator('.charts-panel [role="img"]').last();
  await expect(chart).toHaveAttribute("aria-label", /\b[1-9]\d* recorded forecasts/);
  const description = await chart.getAttribute("aria-label");
  await page.getByRole("button", { name: "Pin chart" }).click();
  await expect(page.locator('.pinned-panel [role="img"]')).toHaveAttribute("aria-label", description!);
});

test("driver dialog opens while the chart renderer is still loading", async ({ page }) => {
  let release: (() => void) | undefined;
  const blocked = new Promise<void>(resolve => { release = resolve; });
  await page.route(/\/src\/EChart\.tsx/, async route => { await blocked; await route.continue(); });
  await page.goto("/");
  await page.getByRole("button", { name: "Open telemetry for VER" }).click();
  await expect(page.getByRole("dialog", { name: "VER telemetry" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Close telemetry" })).toBeVisible();
  release?.();
  await page.getByRole("button", { name: "Close telemetry" }).click();
});
