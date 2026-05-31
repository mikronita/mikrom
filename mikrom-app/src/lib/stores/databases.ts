import { writable } from "svelte/store";

export interface Database {
  id: string;
  name: string;
  version: string;
  status: "Provisioning" | "Running" | "Deleting" | "Stopped";
  vcpus: number;
  memory_mib: number;
  storage_gb: number;
  connection_string: string;
  created_at: string;
  updated_at: string;
}

const mockDatabases: Database[] = [
  {
    id: "db-1",
    name: "prod-db",
    version: "16",
    status: "Running",
    vcpus: 2,
    memory_mib: 4096,
    storage_gb: 50,
    connection_string: "postgresql://mikrom:password@prod-db.mikrom.internal:5432/mikrom",
    created_at: new Date(Date.now() - 1000 * 60 * 60 * 24 * 7).toISOString(), // 7 days ago
    updated_at: new Date(Date.now() - 1000 * 60 * 60 * 24).toISOString(),
  },
  {
    id: "db-2",
    name: "staging-db",
    version: "15",
    status: "Running",
    vcpus: 1,
    memory_mib: 1024,
    storage_gb: 10,
    connection_string: "postgresql://mikrom:password@staging-db.mikrom.internal:5432/mikrom",
    created_at: new Date(Date.now() - 1000 * 60 * 60 * 24 * 2).toISOString(), // 2 days ago
    updated_at: new Date(Date.now() - 1000 * 60 * 60 * 12).toISOString(),
  }
];

export const databasesStore = writable<Database[]>(mockDatabases);
export const databasesLoading = writable<boolean>(false);

export function addDatabase(db: Omit<Database, "id" | "status" | "connection_string" | "created_at" | "updated_at">) {
  const newDb: Database = {
    ...db,
    id: `db-${Math.random().toString(36).substr(2, 9)}`,
    status: "Provisioning",
    connection_string: `postgresql://mikrom:password@${db.name}.mikrom.internal:5432/mikrom`,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
  };

  databasesStore.update(dbs => [newDb, ...dbs]);

  // Simulate provisioning
  setTimeout(() => {
    updateDatabaseStatus(newDb.id, "Running");
  }, 5000);
}

export function deleteDatabase(id: string) {
  databasesStore.update(dbs => dbs.map(db => db.id === id ? { ...db, status: "Deleting" as const } : db));
  
  setTimeout(() => {
    databasesStore.update(dbs => dbs.filter(db => db.id !== id));
  }, 2000);
}

export function updateDatabaseStatus(id: string, status: Database["status"]) {
  databasesStore.update(dbs => dbs.map(db => db.id === id ? { ...db, status, updated_at: new Date().toISOString() } : db));
}
