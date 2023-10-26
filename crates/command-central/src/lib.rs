use std::collections::HashMap;
use lazy_static::lazy_static;
use std::sync::Mutex;

#[derive(Default, Clone)]
pub struct Command {
    title: String,
    docs: String,
}

lazy_static! {
    static ref COMMANDS_MAP: Mutex<HashMap<String, Command>> = Mutex::new(HashMap::new());

}

pub fn add_command(system_name: String, command: Command) {
    let mut commands = COMMANDS_MAP.lock().unwrap();
    commands.insert(system_name, command);
}


pub fn read_command(system_name: String) -> Option<Command> {
    let commands = COMMANDS_MAP.lock().unwrap();
    return commands.get(&system_name).cloned();
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_adds_and_gets_new_command() {
        add_command("test-command".to_string(), Command {
            title: "Test Command".to_string(),
            docs: "Here are some docs about the command".to_string(),
        });
        let command = read_command("test-command".to_string()).unwrap();
        assert_eq!(command.title, "Test Command".to_string());
    }

    #[test]
    fn handles_not_found_commands() {
        // assert_eq!(COMMANDS_MAP.is_none(), false);
    }
}
