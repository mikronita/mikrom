"use client";

import { useState, type FormEvent } from "react";
import { useRouter } from "next/navigation";
import Link from "next/link";
import { ArrowLeft, Loader2, AlertCircle, UserPlus } from "lucide-react";

import { register } from "@/lib/api";
import { Button, Card, Label, TextInput, Alert } from "flowbite-react";

export default function RegisterPage() {
  const router = useRouter();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [error, setError] = useState("");
  const [isLoading, setIsLoading] = useState(false);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setError("");

    if (!email || !password) {
      setError("Email and password are required");
      return;
    }

    if (password.length < 8) {
      setError("Password must be at least 8 characters");
      return;
    }

    if (password !== confirmPassword) {
      setError("Passwords do not match");
      return;
    }

    setIsLoading(true);
    const result = await register({ email, password });
    setIsLoading(false);

    if (result.error) {
      setError(result.error);
    } else if (result.data) {
      router.push("/auth/login?registered=true");
    }
  };

  return (
    <div className="min-h-screen flex flex-col items-center justify-center bg-zinc-50 dark:bg-zinc-950 px-4 relative overflow-hidden">
      {/* Background blobs */}
      <div className="absolute top-0 right-0 w-full h-full -z-10 opacity-30 pointer-events-none">
        <div className="absolute -top-24 -right-24 w-96 h-96 bg-zinc-200 dark:bg-zinc-800 rounded-full blur-[100px]" />
        <div className="absolute -bottom-24 -left-24 w-96 h-96 bg-zinc-200 dark:bg-zinc-800 rounded-full blur-[100px]" />
      </div>

      <Card className="w-full max-w-md shadow-2xl dark:bg-zinc-900 border-zinc-200/50 dark:border-zinc-800/50">
        <div className="space-y-1 text-center">
          <div className="flex justify-center mb-4">
            <div className="w-12 h-12 bg-zinc-900 dark:bg-zinc-50 rounded-2xl flex items-center justify-center shadow-lg">
              <UserPlus className="w-6 h-6 text-white dark:text-zinc-900" />
            </div>
          </div>
          <h2 className="text-2xl font-bold tracking-tight dark:text-white">Create an account</h2>
          <p className="text-sm text-zinc-500 dark:text-zinc-400">
            Enter your details to get started with Mikrom
          </p>
        </div>
        
        <form onSubmit={handleSubmit} className="space-y-4">
          {error && (
            <Alert color="failure" icon={() => <AlertCircle className="w-4 h-4 mr-2" />}>
              {error}
            </Alert>
          )}

          <div>
            <div className="mb-2 block">
              <Label htmlFor="email">Email address</Label>
            </div>
            <TextInput
              id="email"
              type="email"
              placeholder="name@example.com"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              disabled={isLoading}
              required
            />
          </div>

          <div>
            <div className="mb-2 block">
              <Label htmlFor="password">Password</Label>
            </div>
            <TextInput
              id="password"
              type="password"
              placeholder="At least 8 characters"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              disabled={isLoading}
              required
            />
          </div>

          <div>
            <div className="mb-2 block">
              <Label htmlFor="confirmPassword">Confirm Password</Label>
            </div>
            <TextInput
              id="confirmPassword"
              type="password"
              placeholder="Repeat your password"
              value={confirmPassword}
              onChange={(e) => setConfirmPassword(e.target.value)}
              disabled={isLoading}
              required
            />
          </div>

          <div className="flex flex-col gap-4">
            <Button type="submit" color="dark" className="w-full" disabled={isLoading}>
              {isLoading ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  Creating account...
                </>
              ) : (
                "Create Account"
              )}
            </Button>
            <div className="text-center text-sm text-zinc-500">
              Already have an account?{" "}
              <Link href="/auth/login" className="font-semibold text-zinc-900 dark:text-zinc-100 hover:underline">
                Sign in
              </Link>
            </div>
          </div>
        </form>
      </Card>

      <p className="mt-8 text-center text-xs text-zinc-500 max-w-[300px]">
        By clicking continue, you agree to our Terms of Service and Privacy Policy.
      </p>
    </div>
  );
}
