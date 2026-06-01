<script lang="ts">
  import { Card, CardContent, CardDescription, CardHeader, CardTitle, Switch } from "$lib/components";

  let {
    emailNotifications = $bindable(true),
    marketingEmails = $bindable(false),
  } = $props<{
    emailNotifications?: boolean;
    marketingEmails?: boolean;
  }>();
</script>

<Card size="sm">
  <CardHeader>
    <CardTitle>Notifications</CardTitle>
    <CardDescription>Choose what updates you want to receive via email.</CardDescription>
  </CardHeader>
  <CardContent>
    <div class="flex flex-col gap-4">
      <Card size="sm" class="overflow-hidden">
        <CardContent>
          <div class="flex items-start justify-between gap-4">
            <div class="flex flex-col gap-1">
              <div class="text-base font-medium">Deployment status</div>
              <p class="text-sm text-muted-foreground">Receive an email when your deployments finish or fail.</p>
            </div>
            <Switch bind:checked={emailNotifications} aria-label="Toggle deployment status notifications" />
          </div>
        </CardContent>
      </Card>
      <Card size="sm" class="overflow-hidden">
        <CardContent>
          <div class="flex items-start justify-between gap-4">
            <div class="flex flex-col gap-1">
              <div class="text-base font-medium">Marketing emails</div>
              <p class="text-sm text-muted-foreground">New features, tips and weekly summaries.</p>
            </div>
            <Switch bind:checked={marketingEmails} aria-label="Toggle marketing emails" />
          </div>
        </CardContent>
      </Card>
    </div>
  </CardContent>
</Card>
