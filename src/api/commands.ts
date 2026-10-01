import { invoke } from "@tauri-apps/api/core";
import type { CreateProjectInput, Project } from "../bindings/contracts";

export const projects = {
  create(input: CreateProjectInput): Promise<Project> {
    return invoke<Project>("projects_create", { input });
  },

  list(): Promise<Project[]> {
    return invoke<Project[]>("projects_list");
  },
};
