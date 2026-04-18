import {
  Flex,
  Text,
  Card,
  Button,
  Badge,
  RadioGroup,
  Separator,
} from "@radix-ui/themes";
import { ExitIcon } from "@radix-ui/react-icons";
import { useAuth } from "../hooks/useAuth";

interface SettingsProps {
  theme: "dark" | "light";
  setTheme: (t: "dark" | "light") => void;
}

export function Settings({ theme, setTheme }: SettingsProps) {
  const { user, logout } = useAuth();

  return (
    <Flex direction="column" gap="4" style={{ maxWidth: 500 }}>
      <Text size="5" weight="bold">
        Settings
      </Text>

      <Card>
        <Flex direction="column" gap="4">
          <Text size="3" weight="medium">
            Appearance
          </Text>
          <RadioGroup.Root value={theme} onValueChange={(v) => setTheme(v as "dark" | "light")}>
            <Flex direction="column" gap="2">
              <Text as="label" size="2">
                <Flex gap="2" align="center">
                  <RadioGroup.Item value="dark" />
                  Dark
                </Flex>
              </Text>
              <Text as="label" size="2">
                <Flex gap="2" align="center">
                  <RadioGroup.Item value="light" />
                  Light
                </Flex>
              </Text>
            </Flex>
          </RadioGroup.Root>
        </Flex>
      </Card>

      <Card>
        <Flex direction="column" gap="3">
          <Text size="3" weight="medium">
            Account
          </Text>

          <Flex direction="column" gap="2">
            <Flex align="center" gap="2">
              <Text size="2" color="gray">
                Username:
              </Text>
              <Text size="2" weight="medium">
                {user?.username ?? "Unknown"}
              </Text>
              {user?.is_admin && (
                <Badge size="1" color="amber">
                  Admin
                </Badge>
              )}
            </Flex>

            {user?.created_at && (
              <Flex align="center" gap="2">
                <Text size="2" color="gray">
                  Member since:
                </Text>
                <Text size="2">
                  {new Date(user.created_at).toLocaleDateString()}
                </Text>
              </Flex>
            )}
          </Flex>

          <Separator size="4" />

          <Button variant="outline" color="red" onClick={logout}>
            <ExitIcon />
            Logout
          </Button>
        </Flex>
      </Card>
    </Flex>
  );
}
