import { open } from "@tauri-apps/plugin-dialog";

/** Opens the native folder picker. Resolves to `null` when the user cancels. */
export async function chooseProjectFolder(): Promise<string | null> {
  const selection = await open({
    directory: true,
    multiple: false,
    title: "Choose the project folder ThreadBack may collect evidence from",
  });
  return typeof selection === "string" ? selection : null;
}
