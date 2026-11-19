import { expect, test } from "bun:test";
import { customProviderApiOverrides, mergeDiscoveredModels } from "../src/CustomProviderModal";
import type { CustomProviderModel } from "../src/types";

test("catalog refresh preserves a saved transport while updating model capabilities", () => {
  const saved: CustomProviderModel[] = [{
    id: "existing-model", name: "Existing", api: "openai-responses",
    configuration: { contextWindow: 32000, supportsTools: false }
  }];
  const refreshed = mergeDiscoveredModels(saved, [{
    id: "existing-model", name: "Updated", api: "anthropic-messages",
    configuration: { contextWindow: 64000 }
  }, {
    id: "new-model", name: "New", api: "google-generative-ai"
  }]);
  expect(refreshed[0].api).toBe("openai-responses");
  expect(refreshed[0].name).toBe("Updated");
  expect(refreshed[0].configuration).toEqual({ contextWindow: 64000, supportsTools: false });
  expect(refreshed[1].api).toBe("google-generative-ai");
});

test("saved model transports remain explicit even when equal to the editor default", () => {
  const saved: CustomProviderModel[] = [
    { id: "model-a", name: "A", api: "openai-responses" },
    { id: "model-b", name: "B", api: "anthropic-messages" }
  ];
  const expected = { "model-a": "openai-responses", "model-b": "anthropic-messages" };
  expect(customProviderApiOverrides(saved, "openai-responses")).toEqual(expected);
  expect(customProviderApiOverrides(saved, "anthropic-messages")).toEqual(expected);
});
