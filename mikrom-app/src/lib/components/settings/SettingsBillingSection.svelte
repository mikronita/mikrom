<script lang="ts">
  import { Badge, Button, Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "$lib/components";
</script>

<div class="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(20rem,0.85fr)]">
  <Card class="overflow-hidden">
    <CardHeader class="border-b bg-muted/30">
      <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
        <div class="flex flex-col gap-2">
          <CardDescription>Current plan</CardDescription>
          <CardTitle class="text-2xl">Pro developer</CardTitle>
          <p class="text-sm text-muted-foreground">$29 / month</p>
        </div>
        <Badge variant="outline" class="border-transparent bg-status-info/10 text-status-info">Active</Badge>
      </div>
    </CardHeader>
    <CardContent class="pt-6">
      <div class="grid gap-3 sm:grid-cols-3">
        <div class="rounded-2xl border border-border/70 bg-background/70 p-5">
          <p class="text-2xl font-semibold">4</p>
          <p class="text-sm text-muted-foreground">Active apps</p>
        </div>
        <div class="rounded-2xl border border-border/70 bg-background/70 p-5">
          <p class="text-2xl font-semibold">120 GB</p>
          <p class="text-sm text-muted-foreground">Bandwidth</p>
        </div>
        <div class="rounded-2xl border border-border/70 bg-background/70 p-5">
          <p class="text-2xl font-semibold">24/7</p>
          <p class="text-sm text-muted-foreground">Support</p>
        </div>
      </div>
    </CardContent>
    <CardFooter class="flex flex-wrap gap-2">
      <Button size="sm">Change plan</Button>
      <Button variant="outline" size="sm">Cancel subscription</Button>
    </CardFooter>
  </Card>

  <Card>
    <CardHeader>
      <CardTitle>Payment method</CardTitle>
      <CardDescription>Manage the card used for renewals and invoices.</CardDescription>
    </CardHeader>
    <CardContent>
      <Card size="sm" class="overflow-hidden">
        <CardContent>
          <div class="flex items-center gap-4">
            <div class="flex h-8 w-12 shrink-0 items-center justify-center rounded-md border bg-background text-xs font-semibold italic text-muted-foreground">
              VISA
            </div>
            <div class="min-w-0 flex-1">
              <p class="truncate text-sm font-semibold">Visa ending in 4242</p>
              <p class="text-xs text-muted-foreground">Expires 12/28</p>
            </div>
            <Button variant="outline" size="sm">Edit</Button>
          </div>
        </CardContent>
      </Card>
    </CardContent>
  </Card>
</div>
