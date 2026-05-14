"use client";

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  HardDrive,
  Loader2,
  Plus,
  Trash2,
  Database,
  Camera,
  History,
  RotateCcw,
  Copy,
} from "lucide-react";
import { toast } from "sonner";

import { AuthGuard } from "@/components/AuthGuard";
import { DashboardLayout } from "@/components/DashboardLayout";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import {
  Field,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  createVolume,
  createVolumeSnapshot,
  cloneVolumeFromSnapshot,
  deleteVolume,
  deleteVolumeSnapshot,
  listApps,
  listVolumes,
  listVolumeSnapshots,
  restoreVolumeSnapshot,
} from "@/lib/api";
import { Label } from "@/components/ui/label";
import { getToken } from "@/lib/auth";

export default function StoragePage() {
  const token = getToken();
  const queryClient = useQueryClient();

  const cloneSnapshotMutation = useMutation({
    mutationFn: ({ volumeId, name, snapshotName }: { volumeId: string; name: string; snapshotName: string }) =>
      cloneVolumeFromSnapshot(token!, volumeId, { name, snapshot_name: snapshotName }),
    onSuccess: (res) => {
      if (res.error) {
        toast.error(res.error);
        return;
      }
      toast.success("Volume cloned from snapshot");
      setSnapshotToClone(null);
      setCloneName("");
      queryClient.invalidateQueries({ queryKey: ["volumes", selectedApp] });
    },
  });
  const [selectedApp, setSelectedApp] = useState<string | null>(null);
  const [isAddVolumeOpen, setIsAddVolumeOpen] = useState(false);
  const [volumeToDelete, setVolumeToDelete] = useState<string | null>(null);
  const [volumeForSnapshots, setVolumeForSnapshots] = useState<string | null>(null);
  const [isRestoreOpen, setIsRestoreOpen] = useState(false);
  const [snapshotToRestore, setSnapshotToRestore] = useState<{ volumeId: string, name: string } | null>(null);
  const [snapshotToDelete, setSnapshotToDelete] = useState<string | null>(null);
  const [snapshotToClone, setSnapshotToClone] = useState<{ volumeId: string, name: string } | null>(null);
  const [newVolume, setNewVolume] = useState({ name: "", size_mib: 1024 });
  const [cloneName, setCloneName] = useState("");

  const { data: apps } = useQuery({
    queryKey: ["apps"],
    queryFn: () =>
      listApps(token!).then((res) => {
        if (res.error) throw new Error(res.error);
        return res.data;
      }),
    enabled: !!token,
  });

  const { data: volumes, isLoading: volumesLoading } = useQuery({
    queryKey: ["volumes", selectedApp],
    queryFn: () => {
      const app = apps?.find(a => a.name === selectedApp);
      return listVolumes(token!, app!.id).then((res) => {
        if (res.error) throw new Error(res.error);
        return res.data;
      });
    },
    enabled: !!token && !!selectedApp && !!apps,
  });

  const createVolumeMutation = useMutation({
    mutationFn: (data: typeof newVolume) => {
      const app = apps?.find(a => a.name === selectedApp);
      return createVolume(token!, app!.id, data);
    },
    onSuccess: (res) => {
      if (res.error) {
        toast.error(res.error);
        return;
      }

      toast.success("Volume created successfully");
      queryClient.invalidateQueries({ queryKey: ["volumes", selectedApp] });
      setNewVolume({ name: "", size_mib: 1024 });
      setIsAddVolumeOpen(false);
    },
  });

  const deleteVolumeMutation = useMutation({
    mutationFn: (volumeId: string) => deleteVolume(token!, volumeId),
    onSuccess: (res) => {
      if (res.error) {
        toast.error(res.error);
        return;
      }

      toast.success("Volume deleted");
      queryClient.invalidateQueries({ queryKey: ["volumes", selectedApp] });
      setVolumeToDelete(null);
    },
  });

  const createSnapshotMutation = useMutation({
    mutationFn: ({ volumeId, name }: { volumeId: string; name: string }) => 
      createVolumeSnapshot(token!, volumeId, { name }),
    onSuccess: (res, variables) => {
      if (res.error) {
        toast.error(res.error);
        return;
      }
      toast.success("Snapshot created");
      queryClient.invalidateQueries({ queryKey: ["snapshots", variables.volumeId] });
    },
  });

  const { data: snapshots, isLoading: snapshotsLoading } = useQuery({
    queryKey: ["snapshots", volumeForSnapshots],
    queryFn: () => listVolumeSnapshots(token!, volumeForSnapshots!).then((res) => {
      if (res.error) throw new Error(res.error);
      return res.data;
    }),
    enabled: !!token && !!volumeForSnapshots,
  });

  const restoreSnapshotMutation = useMutation({
    mutationFn: ({ volumeId, snapshotName }: { volumeId: string; snapshotName: string }) =>
      restoreVolumeSnapshot(token!, volumeId, { snapshot_name: snapshotName }),
    onSuccess: (res) => {
      if (res.error) {
        toast.error(res.error);
        return;
      }
      toast.success("Volume restored to snapshot");
      setIsRestoreOpen(false);
      setSnapshotToRestore(null);
    },
  });

  const deleteSnapshotMutation = useMutation({
    mutationFn: (snapshotId: string) => deleteVolumeSnapshot(token!, snapshotId),
    onSuccess: (res) => {
      if (res.error) {
        toast.error(res.error);
        return;
      }
      toast.success("Snapshot deleted");
      queryClient.invalidateQueries({ queryKey: ["snapshots", volumeForSnapshots] });
      setSnapshotToDelete(null);
    },
  });

  return (
    <AuthGuard>
      <DashboardLayout>
        <div className="flex flex-col gap-6">
          <div className="flex flex-col gap-4 md:flex-row md:items-end md:justify-between">
            <div className="flex flex-col gap-2">
              <div className="flex items-center gap-3">
                <div className="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
                  <HardDrive />
                </div>
                <h1 className="text-3xl font-semibold tracking-tight">Storage</h1>
              </div>
              <p className="max-w-2xl text-sm text-muted-foreground">
                Manage persistent block storage (Ceph RBD) for your applications.
              </p>
            </div>
          </div>

          <div className="grid gap-4">
            <Card className="overflow-hidden">
              <CardHeader className="border-b bg-muted/20">
                <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
                  <div className="flex flex-col gap-1.5">
                    <CardTitle className="flex items-center gap-2 text-lg">
                      <Database />
                      Volumes
                    </CardTitle>
                    <CardDescription>
                      Persistent volumes can be attached to your microVMs.
                    </CardDescription>
                  </div>
                  <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
                    <Select onValueChange={(value) => setSelectedApp(value)}>
                      <SelectTrigger className="sm:w-[220px]">
                        <SelectValue placeholder="Select application" />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectGroup>
                          {apps?.map((app) => (
                            <SelectItem key={app.id} value={app.name}>
                              {app.name}
                            </SelectItem>
                          ))}
                        </SelectGroup>
                      </SelectContent>
                    </Select>
                    
                    {selectedApp && (
                      <Dialog open={isAddVolumeOpen} onOpenChange={setIsAddVolumeOpen}>
                        <DialogTrigger asChild>
                          <Button size="sm">
                            <Plus data-icon="inline-start" />
                            Create Volume
                          </Button>
                        </DialogTrigger>
                        <DialogContent>
                          <DialogHeader>
                            <DialogTitle>Create new volume</DialogTitle>
                            <DialogDescription>
                              The volume will be created in the Ceph cluster and will be available for <strong>{selectedApp}</strong>.
                            </DialogDescription>
                          </DialogHeader>
                          <FieldGroup>
                            <Field>
                              <FieldLabel htmlFor="name">Volume Name</FieldLabel>
                              <Input
                                id="name"
                                placeholder="my-data-volume"
                                value={newVolume.name}
                                onChange={(e) => setNewVolume({ ...newVolume, name: e.target.value })}
                              />
                            </Field>
                            <Field>
                              <FieldLabel htmlFor="size">Size (MiB)</FieldLabel>
                              <Input
                                id="size"
                                type="number"
                                min={128}
                                value={newVolume.size_mib}
                                onChange={(e) => setNewVolume({ ...newVolume, size_mib: parseInt(e.target.value) || 0 })}
                              />
                            </Field>
                          </FieldGroup>
                          <DialogFooter>
                            <Button
                              onClick={() => createVolumeMutation.mutate(newVolume)}
                              disabled={createVolumeMutation.isPending || !newVolume.name}
                            >
                              {createVolumeMutation.isPending && (
                                <Loader2 data-icon="inline-start" className="animate-spin" />
                              )}
                              Create
                            </Button>
                          </DialogFooter>
                        </DialogContent>
                      </Dialog>
                    )}
                  </div>
                </div>
              </CardHeader>
              <CardContent className="p-0">
                {!selectedApp ? (
                  <Empty className="border-0 py-14">
                    <EmptyHeader>
                      <EmptyMedia variant="icon">
                        <Database />
                      </EmptyMedia>
                      <EmptyTitle>Select an application</EmptyTitle>
                      <EmptyDescription>
                        Choose an app to manage its persistent storage volumes.
                      </EmptyDescription>
                    </EmptyHeader>
                  </Empty>
                ) : volumesLoading ? (
                  <div className="flex flex-col gap-3 p-6">
                    <Skeleton className="h-10 w-full" />
                    <Skeleton className="h-10 w-full" />
                  </div>
                ) : volumes?.length === 0 ? (
                  <Empty className="border-0 py-14">
                    <EmptyHeader>
                      <EmptyMedia variant="icon">
                        <HardDrive />
                      </EmptyMedia>
                      <EmptyTitle>No volumes found</EmptyTitle>
                      <EmptyDescription>
                        This application doesn&apos;t have any persistent volumes yet.
                      </EmptyDescription>
                    </EmptyHeader>
                    <EmptyContent>
                      <Button size="sm" onClick={() => setIsAddVolumeOpen(true)}>
                        <Plus data-icon="inline-start" />
                        Create first volume
                      </Button>
                    </EmptyContent>
                  </Empty>
                ) : (
                  <Table>
                    <TableHeader>
                      <TableRow className="hover:bg-transparent">
                        <TableHead className="px-6">Name</TableHead>
                        <TableHead>Size</TableHead>
                        <TableHead>Pool</TableHead>
                        <TableHead>Created At</TableHead>
                        <TableHead className="pr-6 text-right">Actions</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {volumes?.map((volume) => (
                        <TableRow key={volume.id}>
                          <TableCell className="px-6 font-medium">
                            <div className="flex items-center gap-2">
                              <Database className="size-4 text-muted-foreground" />
                              {volume.name}
                            </div>
                          </TableCell>
                          <TableCell>
                            <Badge variant="secondary">
                              {volume.size_mib >= 1024 
                                ? `${(volume.size_mib / 1024).toFixed(1)} GiB` 
                                : `${volume.size_mib} MiB`}
                            </Badge>
                          </TableCell>
                          <TableCell className="font-mono text-xs text-muted-foreground">
                            {volume.pool_name}
                          </TableCell>
                          <TableCell className="text-sm text-muted-foreground">
                            {new Date(volume.created_at).toLocaleDateString()}
                          </TableCell>
                          <TableCell className="pr-6 text-right">
                            <div className="flex justify-end gap-2">
                              <Button
                                variant="ghost"
                                size="icon"
                                onClick={() => setVolumeForSnapshots(volume.id)}
                                title="Snapshot history"
                              >
                                <History className="size-4" />
                              </Button>
                              <Button
                                variant="ghost"
                                size="icon"
                                onClick={() => {
                                  const snapName = `snap-${new Date().toISOString().replace(/[:.]/g, "-")}`;
                                  createSnapshotMutation.mutate({ volumeId: volume.id, name: snapName });
                                }}
                                disabled={createSnapshotMutation.isPending}
                                title="Create snapshot"
                              >
                                <Camera className="size-4" />
                              </Button>
                              <Button
                                variant="ghost"
                                size="icon"
                                className="text-destructive hover:bg-destructive/10"
                                onClick={() => setVolumeToDelete(volume.id)}
                                disabled={deleteVolumeMutation.isPending}
                              >
                                <Trash2 className="size-4" />
                              </Button>
                            </div>
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                )}
              </CardContent>
            </Card>
          </div>
        </div>

        <AlertDialog open={!!volumeToDelete} onOpenChange={(open) => !open && setVolumeToDelete(null)}>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>Are you absolutely sure?</AlertDialogTitle>
              <AlertDialogDescription>
                This action cannot be undone. This will permanently delete the persistent volume
                and all data contained within it from the Ceph cluster.
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>Cancel</AlertDialogCancel>
              <AlertDialogAction
                className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
                onClick={() => volumeToDelete && deleteVolumeMutation.mutate(volumeToDelete)}
              >
                Delete Volume
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>

        <Dialog open={!!volumeForSnapshots} onOpenChange={(open) => !open && setVolumeForSnapshots(null)}>
          <DialogContent className="max-w-2xl">
            <DialogHeader>
              <DialogTitle>Snapshot History</DialogTitle>
              <DialogDescription>
                Manage snapshots for volume <strong>{volumes?.find(v => v.id === volumeForSnapshots)?.name}</strong>.
              </DialogDescription>
            </DialogHeader>
            
            <div className="py-4">
              {snapshotsLoading ? (
                <div className="flex items-center justify-center p-8">
                  <Loader2 className="animate-spin text-muted-foreground" />
                </div>
              ) : snapshots?.length === 0 ? (
                <div className="text-center py-8 text-muted-foreground italic">
                  No snapshots found for this volume.
                </div>
              ) : (
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Name</TableHead>
                      <TableHead>Created At</TableHead>
                      <TableHead className="text-right">Actions</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {snapshots?.map((snap) => (
                      <TableRow key={snap.id}>
                        <TableCell className="font-medium">{snap.name}</TableCell>
                        <TableCell className="text-sm text-muted-foreground">
                          {new Date(snap.created_at).toLocaleString()}
                        </TableCell>
                        <TableCell className="text-right">
                          <div className="flex justify-end gap-2">
                            <Button
                              variant="outline"
                              size="sm"
                              onClick={() => {
                                setSnapshotToRestore({ volumeId: snap.volume_id, name: snap.name });
                                setIsRestoreOpen(true);
                              }}
                            >
                              <RotateCcw data-icon="inline-start" className="size-3" />
                              Restore
                            </Button>
                            <Button
                              variant="outline"
                              size="sm"
                              onClick={() => {
                                setSnapshotToClone({ volumeId: snap.volume_id, name: snap.name });
                                setCloneName(`${volumes?.find(v => v.id === snap.volume_id)?.name}-clone`);
                              }}
                            >
                              <Copy data-icon="inline-start" className="size-3" />
                              Clone
                            </Button>
                            <Button
                              variant="ghost"
                              size="icon"
                              className="text-destructive hover:bg-destructive/10"
                              onClick={() => setSnapshotToDelete(snap.id)}
                              disabled={deleteSnapshotMutation.isPending}
                            >
                              <Trash2 className="size-4" />
                            </Button>
                          </div>
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              )}
            </div>
          </DialogContent>
        </Dialog>

        <AlertDialog open={isRestoreOpen} onOpenChange={setIsRestoreOpen}>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>Restore volume snapshot?</AlertDialogTitle>
              <AlertDialogDescription>
                This will revert the volume to the state it was in when the snapshot <strong>{snapshotToRestore?.name}</strong> was taken. 
                <span className="mt-2 block font-semibold text-destructive">Any data written after the snapshot was created will be lost.</span>
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel onClick={() => setSnapshotToRestore(null)}>Cancel</AlertDialogCancel>
              <AlertDialogAction
                onClick={() => snapshotToRestore && restoreSnapshotMutation.mutate({ 
                  volumeId: snapshotToRestore.volumeId, 
                  snapshotName: snapshotToRestore.name 
                })}
                disabled={restoreSnapshotMutation.isPending}
              >
                {restoreSnapshotMutation.isPending && (
                  <Loader2 data-icon="inline-start" className="animate-spin" />
                )}
                Confirm Restore
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>

        <AlertDialog open={!!snapshotToDelete} onOpenChange={(open) => !open && setSnapshotToDelete(null)}>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>Delete snapshot?</AlertDialogTitle>
              <AlertDialogDescription>
                This will permanently delete the snapshot from the Ceph cluster. This action cannot be undone.
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>Cancel</AlertDialogCancel>
              <AlertDialogAction
                className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
                onClick={() => snapshotToDelete && deleteSnapshotMutation.mutate(snapshotToDelete)}
              >
                {deleteSnapshotMutation.isPending && (
                  <Loader2 data-icon="inline-start" className="animate-spin" />
                )}
                Delete Snapshot
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>

        {/* Clone Volume Dialog */}
        <Dialog open={!!snapshotToClone} onOpenChange={(open) => !open && setSnapshotToClone(null)}>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Clone Volume</DialogTitle>
              <DialogDescription>
                Create a new volume from snapshot <strong>{snapshotToClone?.name}</strong>.
              </DialogDescription>
            </DialogHeader>
            <div className="grid gap-4 py-4">
              <div className="grid gap-2">
                <Label htmlFor="clone-name">New Volume Name</Label>
                <Input
                  id="clone-name"
                  value={cloneName}
                  onChange={(e) => setCloneName(e.target.value)}
                  placeholder="my-cloned-volume"
                />
              </div>
            </div>
            <DialogFooter>
              <Button variant="outline" onClick={() => setSnapshotToClone(null)}>
                Cancel
              </Button>
              <Button
                onClick={() => {
                  if (!cloneName) {
                    toast.error("Please provide a name for the new volume");
                    return;
                  }
                  cloneSnapshotMutation.mutate({
                    volumeId: snapshotToClone!.volumeId,
                    name: cloneName,
                    snapshotName: snapshotToClone!.name,
                  });
                }}
                disabled={cloneSnapshotMutation.isPending}
              >
                {cloneSnapshotMutation.isPending ? "Cloning..." : "Clone Volume"}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </DashboardLayout>
    </AuthGuard>
  );
}
