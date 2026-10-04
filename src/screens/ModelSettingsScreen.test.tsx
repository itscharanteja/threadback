import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FakeBackend } from "../test/fakeBackend";
import { ModelSettingsScreen } from "./ModelSettingsScreen";

const bridge = vi.hoisted(() => ({
  invoke: null as null | ((command: string, args?: unknown) => Promise<unknown>),
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args?: unknown) => bridge.invoke!(command, args),
}));

const KEY = "tfk-7F4A91C2-demo-secret-key-value-0001";
let backend: FakeBackend;

async function open() {
  backend = new FakeBackend("ACTIVE");
  bridge.invoke = backend.invoke;
  const user = userEvent.setup();
  render(<ModelSettingsScreen />);
  await screen.findByText(/No API key is stored yet|stored in the macOS Keychain/);
  return user;
}

const field = (name: RegExp) => screen.getByLabelText(name);
const button = (name: RegExp | string) => screen.getByRole("button", { name });

beforeEach(() => {
  backend = new FakeBackend("ACTIVE");
});

describe("the model connection screen", () => {
  it("starts with the Token Factory defaults and no key", async () => {
    await open();
    expect(field(/^Endpoint/)).toHaveValue("https://api.tokenfactory.nebius.com/v1");
    expect(field(/^Model ID/)).toHaveValue("nvidia/nemotron-3-super-120b-a12b");
    expect(field(/^Timeout/)).toHaveValue("60");
    expect(screen.getByText("No API key is stored yet.")).toBeInTheDocument();
    expect(screen.getByText(/Evidence would be sent to/)).toHaveTextContent("api.tokenfactory.nebius.com");
    expect(screen.queryByRole("button", { name: /Remove stored key/ })).toBeNull();
  });

  it("takes the key in a password field that is never filled in", async () => {
    await open();
    const key = field(/^API key/);
    expect(key).toHaveAttribute("type", "password");
    expect(key).toHaveAttribute("autocomplete", "off");
    expect(key).toHaveValue("");
    expect(screen.getByText(/Kept only in the macOS Keychain/)).toBeInTheDocument();
  });

  it("saves the key once, then clears it and never shows it again", async () => {
    const user = await open();
    await user.type(field(/^API key/), KEY);
    await user.click(button("Save"));

    expect(await screen.findByText("Saved. The API key is now in the macOS Keychain.")).toBeInTheDocument();
    expect(backend.callsTo("configuration_store")[0].args.input.apiKey).toBe(KEY);
    expect(screen.getByText("An API key is stored in the macOS Keychain.")).toBeInTheDocument();

    // Nothing on the page, including field values, contains it.
    expect(field(/^Replace the API key/)).toHaveValue("");
    expect(document.body.innerHTML).not.toContain(KEY);
    expect(document.body.innerHTML).not.toContain("7F4A91C2");
    // And nothing the backend returned carried it.
    const status = (await backend.invoke("configuration_status")) as Record<string, unknown>;
    expect(JSON.stringify(status)).not.toContain(KEY);
  });

  it("offers to replace or remove a stored key, and keeps it when saving other changes", async () => {
    const user = await open();
    await user.type(field(/^API key/), KEY);
    await user.click(button("Save"));
    await screen.findByText(/Saved\. The API key is now/);

    expect(field(/^Replace the API key/)).toHaveAttribute("placeholder", "Leave empty to keep the stored key");
    await user.clear(field(/^Timeout/));
    await user.type(field(/^Timeout/), "90");
    await user.click(button("Save"));
    await waitFor(() => expect(backend.callsTo("configuration_store")).toHaveLength(2));
    expect(backend.callsTo("configuration_store")[1].args.input.apiKey).toBeNull();
    expect(await screen.findByText("An API key is stored in the macOS Keychain.")).toBeInTheDocument();
    expect(backend.config.timeoutSecs).toBe(90);

    await user.click(button(/Remove stored key/));
    expect(await screen.findByText("The API key was removed from the Keychain.")).toBeInTheDocument();
    expect(screen.getByText("No API key is stored yet.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Remove stored key/ })).toBeNull();
  });

  it("sends the key only when saving or testing, never otherwise", async () => {
    const user = await open();
    await user.type(field(/^API key/), KEY);
    await user.click(button("Test connection"));
    await screen.findByText(/Connected\./);
    await user.click(button("Save"));
    await screen.findByText(/Saved\./);
    await user.click(button("Test connection"));
    await screen.findByText(/Connected\./);

    const carriers = backend.keysSent.map((sent) => sent.command);
    expect(new Set(carriers)).toEqual(new Set(["configuration_test", "configuration_store"]));
    for (const call of backend.calls) {
      if (!["configuration_test", "configuration_store"].includes(call.command)) {
        expect(JSON.stringify(call.args ?? null)).not.toContain(KEY);
      }
    }
    // After saving, the second test did not resend the key.
    expect(backend.callsTo("configuration_test")[1].args.input.apiKey).toBeNull();
  });

  it("explains a rejected value on the field and moves focus to it", async () => {
    const user = await open();
    await user.clear(field(/^Endpoint/));
    await user.type(field(/^Endpoint/), "http://example.com/v1");
    await user.click(button("Save"));
    expect(await screen.findByText("The endpoint must use https.")).toBeInTheDocument();
    expect(field(/^Endpoint/)).toHaveFocus();
    expect(field(/^Endpoint/)).toHaveAttribute("aria-invalid", "true");
    expect(backend.config.endpoint).toBe("https://api.tokenfactory.nebius.com/v1");
  });

  it("checks the timeout is a number before asking the backend", async () => {
    const user = await open();
    await user.clear(field(/^Timeout/));
    await user.type(field(/^Timeout/), "soon");
    await user.click(button("Save"));
    expect(await screen.findByText("Enter the timeout as a whole number of seconds.")).toBeInTheDocument();
    expect(field(/^Timeout/)).toHaveFocus();
    expect(backend.callsTo("configuration_store")).toHaveLength(0);
  });

  it("shows the backend's reason for a model ID or timeout it refuses", async () => {
    const user = await open();
    await user.clear(field(/^Timeout/));
    await user.type(field(/^Timeout/), "3");
    await user.click(button("Save"));
    expect(await screen.findByText("The timeout must be between 5 and 300 seconds.")).toBeInTheDocument();
  });

  it("tests a connection with the values in the form, without saving", async () => {
    const user = await open();
    await user.type(field(/^API key/), KEY);
    await user.clear(field(/^Model ID/));
    await user.type(field(/^Model ID/), "nvidia/other-model");
    await user.click(button("Test connection"));

    expect(await screen.findByText("Connected. nvidia/other-model answered in 412 ms.")).toBeInTheDocument();
    expect(backend.callsTo("configuration_store")).toHaveLength(0);
    expect(backend.config.modelId).toBe("nvidia/nemotron-3-super-120b-a12b");
    expect(screen.getByText(/contains none of your evidence/)).toBeInTheDocument();
  });

  it("reports a failed test as an alert, with what to fix", async () => {
    const user = await open();
    backend.testResult = { ok: false, message: "The model service rejected the API key. Check it in Settings.", latencyMs: 90, modelId: "m" };
    await user.type(field(/^API key/), KEY);
    await user.click(button("Test connection"));
    expect(await screen.findByRole("alert")).toHaveTextContent("rejected the API key");
    // The typed key stays in the field so it can be corrected, but it is in no text.
    expect(document.body.textContent).not.toContain(KEY);
    expect(field(/^API key/)).toHaveValue(KEY);
  });

  it("says so when testing with no key at all", async () => {
    const user = await open();
    await user.click(button("Test connection"));
    expect(await screen.findByRole("alert")).toHaveTextContent("Enter an API key first. Nothing was sent.");
  });
});
