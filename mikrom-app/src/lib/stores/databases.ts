import { writable } from "svelte/store";
import {
  createDatabase as apiCreateDatabase,
  deleteDatabase as apiDeleteDatabase,
  getDatabaseConnection as apiGetDatabaseConnection,
  listDatabases,
  type CreateDatabaseRequest,
  type DatabaseConnectionInfo,
  type DatabaseInfo,
} from "$lib/api";
import { getToken } from "$lib/auth";

export type DatabaseStatus = "Provisioning" | "Running" | "Deleting" | "Failed";

export interface Database extends Omit<DatabaseInfo, "status"> {
  status: DatabaseStatus;
}

export const databasesStore = writable<Database[]>([]);
export const databasesLoading = writable<boolean>(false);
export const databasesError = writable<string>("");

function mapDatabaseStatus(status: DatabaseInfo["status"]): DatabaseStatus {
  switch (status) {
    case "running":
      return "Running";
    case "deleting":
      return "Deleting";
    case "failed":
      return "Failed";
    default:
      return "Provisioning";
  }
}

function mapDatabase(database: DatabaseInfo): Database {
  return {
    ...database,
    status: mapDatabaseStatus(database.status),
  };
}

function sortDatabases(databases: Database[]) {
  return [...databases].sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime());
}

export function clearDatabases() {
  databasesStore.set([]);
  databasesLoading.set(false);
  databasesError.set("");
}

export async function refreshDatabases() {
  const token = getToken();
  if (!token) {
    clearDatabases();
    return;
  }

  databasesLoading.set(true);
  try {
    const result = await listDatabases(token);
    if (result.error) {
      databasesError.set(result.error);
      return;
    }

    databasesStore.set(sortDatabases((result.data ?? []).map(mapDatabase)));
    databasesError.set("");
  } catch (error) {
    databasesError.set(error instanceof Error ? error.message : "Failed to fetch databases");
  } finally {
    databasesLoading.set(false);
  }
}

export async function createDatabase(request: CreateDatabaseRequest) {
  const token = getToken();
  if (!token) {
    return { error: "You must be logged in" };
  }

  const result = await apiCreateDatabase(token, request);
  if (result.error || !result.data) {
    return { error: result.error ?? "Failed to create database" };
  }

  const created = mapDatabase(result.data);
  databasesStore.update((current) => sortDatabases([created, ...current.filter((db) => db.id !== created.id)]));
  return { data: created };
}

export async function deleteDatabase(databaseId: string) {
  const token = getToken();
  if (!token) {
    return { success: false, error: "You must be logged in" };
  }

  const result = await apiDeleteDatabase(token, databaseId);
  if (!result.success) {
    return result;
  }

  databasesStore.update((current) => current.filter((database) => database.id !== databaseId));
  return result;
}

export async function getDatabaseConnection(databaseId: string): Promise<{ data?: DatabaseConnectionInfo; error?: string }> {
  const token = getToken();
  if (!token) {
    return { error: "You must be logged in" };
  }

  return apiGetDatabaseConnection(token, databaseId);
}
