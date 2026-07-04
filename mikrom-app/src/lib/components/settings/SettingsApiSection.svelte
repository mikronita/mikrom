<script lang="ts">
  import { Button, Card, CardContent, CardDescription, CardHeader, CardTitle } from "$lib/components";
    import CheckCircle2 from "lucide-svelte/icons/check-circle-2";
  import Plus from "lucide-svelte/icons/plus";
</script>

<Card size="sm">
  <CardHeader class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
    <div class="flex flex-col gap-1.5">
      <CardTitle>Personal access tokens</CardTitle>
      <CardDescription>Use tokens to authenticate with the Mikrom CLI and API.</CardDescription>
    </div>
    <Button size="sm">
      <Plus class="size-4" />
      Create token
    </Button>
  </CardHeader>
  <CardContent>
    <div class="flex flex-col gap-4">
      <Card size="sm" class="overflow-hidden">
        <CardContent>
          <div class="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
            <div class="flex items-center gap-4">
              <div class="flex size-10 items-center justify-center rounded-md border border-border bg-background text-foreground">
                <CheckCircle2 class="size-4" />
              </div>
              <div class="min-w-0">
                <p class="truncate font-mono text-sm font-semibold">mikrom_pk_live_****************</p>
                <p class="text-xs text-muted-foreground">Last used 2 hours ago - Created April 12, 2026</p>
              </div>
            </div>
            <Button variant="destructive" size="sm">Revoke</Button>
          </div>
        </CardContent>
      </Card>
    </div>
  </CardContent>
</Card>
