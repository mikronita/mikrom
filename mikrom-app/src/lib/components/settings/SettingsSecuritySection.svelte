<script lang="ts">
  import { Badge, Button, Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle, Field, Input } from "$lib/components";
    import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import Trash2 from "@lucide/svelte/icons/trash-2";
</script>

<div class="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(20rem,0.8fr)]">
  <Card size="sm">
    <CardHeader>
      <CardTitle>Change password</CardTitle>
      <CardDescription>Use a strong password that you do not use anywhere else.</CardDescription>
    </CardHeader>
    <CardContent>
      <div class="flex max-w-md flex-col gap-4">
        <Field label="Current password">
          <Input type="password" />
        </Field>
        <Field label="New password">
          <Input type="password" />
        </Field>
      </div>
    </CardContent>
    <CardFooter>
      <Button>Update password</Button>
    </CardFooter>
  </Card>

  <div class="flex flex-col gap-4">
    <Card size="sm">
      <CardHeader class="flex flex-row items-start justify-between gap-4">
        <div class="flex flex-col gap-1.5">
          <CardTitle>Two-factor authentication</CardTitle>
          <CardDescription>Add an extra layer of security to your account.</CardDescription>
        </div>
        <Badge variant="outline">
          <ShieldCheck class="size-4" />
          Not enabled
        </Badge>
      </CardHeader>
      <CardFooter>
        <Button size="sm">Configure 2FA</Button>
      </CardFooter>
    </Card>

    <Card size="sm">
      <CardHeader>
        <CardTitle class="text-destructive">Danger zone</CardTitle>
        <CardDescription>Once you delete your account, there is no going back. Please be certain.</CardDescription>
      </CardHeader>
      <CardFooter>
        <Button variant="destructive" size="sm">
          <Trash2 class="size-4" />
          Delete account
        </Button>
      </CardFooter>
    </Card>
  </div>
</div>
